//! [`WanderGoal`]: pick a cell somewhere in the neighbourhood, walk to it,
//! pick another.
//!
//! What a wanderer used to be in its entirety, moved up out of the walker
//! because every part of it is a decision: where to go, what to do when there
//! is no getting there, and how long to put up with somebody in the way.

use rand::rngs::SmallRng;
use rand::RngExt;

use crate::map::Point;
use crate::sim::rng::tick_rng;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{PATIENCE, WAIT_FOR_A_GAP};

/// How far a wanderer will look for somewhere to go, in cells.
pub const WANDER_RADIUS: i32 = 6;

/// How many cells to try before giving up and standing still this tick.
pub const WANDER_TRIES: u32 = 8;

/// How often a stuck entity is allowed to mention it. Roughly a second at the
/// fixed timestep.
const STUCK_LOG_TICKS: u64 = 64;

/// Wander about.
///
/// Two ways to miss, and they are deliberately treated differently — the
/// distinction `Walker::plan` used to draw, kept:
///
/// * **The cheap miss.** A few random candidates all landed on a wall or off
///   the map. No search ran, so it costs nothing to try again next tick, and
///   this returns [`GoalProgress::Working`] with nothing queued. Holding off
///   for an unlucky dice roll would make a wanderer stand still for half a
///   second on a map with plenty to walk to.
/// * **The expensive miss.** The candidate *was* passable, and the route to it
///   flooded everything reachable before finding out there was none. That is
///   [`GoalProgress::Blocked`], which holds the goal off.
///
/// And a third thing that is not a miss at all: blocked by a *body*. Bodies
/// move, so it waits a moment and walks the same route again — up to
/// [`PATIENCE`] times — rather than throwing away where it was going.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WanderGoal {
    /// Where it has decided to go. Kept across being put down — a cell
    /// decided on is not stale, where a half-walked route is.
    target: Option<Point>,
    /// Walks to `target` blocked by a body so far, in a row.
    retries: u8,
}

impl WanderGoal {
    pub fn new() -> WanderGoal {
        WanderGoal::default()
    }

    pub fn target(&self) -> Option<Point> {
        self.target
    }

    /// A passable cell within [`WANDER_RADIUS`], or `None` if a few tries
    /// found nothing.
    ///
    /// Bounded tries rather than a scan: this runs per idle entity, and a
    /// bounded miss costs one wasted tick while a scan of the neighbourhood
    /// would cost the frame. Reachability is not checked here and cannot
    /// cheaply be — that is what the route asked for next is for.
    fn pick(ctx: &GoalCtx<'_>, rng: &mut SmallRng) -> Option<Point> {
        let here = ctx.body.center_position();
        for _ in 0..WANDER_TRIES {
            let candidate = Point::new(
                here.x + rng.random_range(-WANDER_RADIUS..=WANDER_RADIUS),
                here.y + rng.random_range(-WANDER_RADIUS..=WANDER_RADIUS),
            );
            if candidate != here && ctx.think.is_passable(candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// Say so when there is nowhere to go, at most once a second.
    ///
    /// Rate-limited on the tick rather than on a stored counter so that a
    /// crowd of stuck wanderers does not turn the log into one line each per
    /// tick, which is how a log becomes noise.
    fn report_stuck(ctx: &GoalCtx<'_>) {
        if ctx.think.tick.is_multiple_of(STUCK_LOG_TICKS) {
            ctx.think
                .log
                .push(format!("{} has nowhere to go", ctx.body.uid()));
        }
    }

    fn forget(&mut self) {
        self.target = None;
        self.retries = 0;
    }
}

impl GoalExecutor for WanderGoal {
    fn goal(&self) -> GoalId {
        GoalId::Wander
    }

    /// Carry on to where it was going — if that is still somewhere nearby.
    /// Put down to go and eat, a wanderer may be across the map from the cell
    /// it had picked, and walking all the way back to it is not wandering.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        let here = ctx.body.center_position();
        match self.target {
            Some(target) if here.manhattan_distance(target) <= WANDER_RADIUS * 2 => {
                let _ = ctx.tasks.push_front(Task::MoveTo { cell: target });
            }
            _ => self.forget(),
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        match last {
            TaskResult::Success => {
                if !ctx.tasks.is_empty() {
                    // The wait before a retry, over; the walk is next.
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Achieved;
            }
            TaskResult::Failed => {
                ctx.tasks.clear();
                match (self.target, ctx.blocked_by) {
                    // Somebody in the way: give them a moment, then go again.
                    (Some(target), Some(_)) if self.retries < PATIENCE => {
                        self.retries += 1;
                        let _ = ctx.tasks.push_front(Task::Wait {
                            seconds: WAIT_FOR_A_GAP,
                        });
                        let _ = ctx.tasks.push_back(Task::MoveTo { cell: target });
                        return GoalProgress::Working;
                    }
                    // Out of patience with them: somewhere else, next tick.
                    (_, Some(_)) => {
                        self.forget();
                        return GoalProgress::Working;
                    }
                    // No way there at all — the expensive miss.
                    (_, None) => {
                        self.forget();
                        Self::report_stuck(ctx);
                        return GoalProgress::Blocked;
                    }
                }
            }
            TaskResult::InProgress | TaskResult::Executing => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        let mut rng = tick_rng(ctx.body.uid(), ctx.think.tick);
        match Self::pick(ctx, &mut rng) {
            Some(target) => {
                self.target = Some(target);
                self.retries = 0;
                let _ = ctx.tasks.push_back(Task::MoveTo { cell: target });
            }
            None => Self::report_stuck(ctx),
        }
        GoalProgress::Working
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "target",
            match self.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "none".to_string(),
            },
        )]
    }
}
