//! [`ProwlGoal`]: a deathdog with nobody to hunt circles the walls, waiting
//! for somebody to come out.
//!
//! Its wander: a few random cells nearby ([`PROWL_RADIUS`]), the first that
//! is off the site, passable, reachable and in the band round the site it
//! lurks in ([`LURK_NEAR`] to [`LURK_FAR`] out) — or, for one further out
//! than that, any nearer the site than it is now — then a walk there and a
//! short stand. A miss is a few cell reads, so it is tried again next tick
//! ([`GoalProgress::Working`]); somebody in the way is waited for, and no way
//! there at all is [`GoalProgress::Blocked`], as a wanderer's is.

use rand::RngExt;

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::deathdogs::{distance_from_site, LURK_FAR, LURK_NEAR};
use crate::sim::rng::tick_rng;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{PATIENCE, WAIT_FOR_A_GAP};

/// How far a prowling deathdog looks for somewhere to go next, in cells.
pub const PROWL_RADIUS: i32 = 12;

/// How many cells it tries before standing still this tick.
pub const PROWL_TRIES: u32 = 8;

/// How long it stands and sniffs the air at each stop, in watched seconds.
pub const LURK: f32 = watched(2.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ProwlGoal {
    target: Option<Point>,
    retries: u8,
}

impl ProwlGoal {
    pub fn new() -> ProwlGoal {
        ProwlGoal::default()
    }

    /// Somewhere to prowl to next, or `None` if a few tries found nothing.
    fn pick(ctx: &GoalCtx<'_>) -> Option<Point> {
        let map = ctx.think.map;
        let here = ctx.body.center_position();
        let now = distance_from_site(map, here);
        let navigation = map.navigation();
        let mut rng = tick_rng(ctx.body.uid(), ctx.think.tick);
        for _ in 0..PROWL_TRIES {
            let candidate = here.offset(
                rng.random_range(-PROWL_RADIUS..=PROWL_RADIUS),
                rng.random_range(-PROWL_RADIUS..=PROWL_RADIUS),
            );
            let out = distance_from_site(map, candidate);
            let in_band = (LURK_NEAR..=LURK_FAR).contains(&out) || (now > LURK_FAR && out > 0 && out < now);
            if candidate != here && in_band && map.is_passable(candidate) && navigation.may_reach(here, candidate) {
                return Some(candidate);
            }
        }
        None
    }

    fn forget(&mut self) {
        self.target = None;
        self.retries = 0;
    }
}

impl GoalExecutor for ProwlGoal {
    fn goal(&self) -> GoalId {
        GoalId::Prowl
    }

    fn save(&self) -> pb::Executor {
        let ProwlGoal { target, retries } = *self;
        pb::Executor::Wander(pb::Wander { target: target.map(Into::into), retries: retries as u32 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Wander(prowl) = saved else {
            return invalid("prowling is a target and its retries");
        };
        let Ok(retries) = u8::try_from(prowl.retries) else {
            return invalid(format!("prowl: {} retries", prowl.retries));
        };
        self.target = prowl.target.map(Into::into);
        self.retries = retries;
        Ok(())
    }

    /// A target picked before a hunt is stale where the hunt has left it.
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.forget();
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        match last {
            TaskResult::Success if !ctx.tasks.is_empty() => return GoalProgress::Working,
            TaskResult::Success => {
                self.forget();
                return GoalProgress::Achieved;
            }
            TaskResult::Failed => {
                ctx.tasks.clear();
                match (self.target, ctx.blocked_by) {
                    (Some(target), Some(_)) if self.retries < PATIENCE => {
                        self.retries += 1;
                        let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                        let _ = ctx.tasks.push_back(Task::move_to(target));
                        return GoalProgress::Working;
                    }
                    (_, Some(_)) => {
                        self.forget();
                        return GoalProgress::Working;
                    }
                    (_, None) => {
                        self.forget();
                        return GoalProgress::Blocked;
                    }
                }
            }
            TaskResult::InProgress | TaskResult::Executing => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        if let Some(target) = Self::pick(ctx) {
            self.target = Some(target);
            self.retries = 0;
            let _ = ctx.tasks.push_back(Task::move_to(target));
            let _ = ctx.tasks.push_back(Task::wait(LURK));
        }
        GoalProgress::Working
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "prowling to",
            match self.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "nowhere yet".to_string(),
            },
        )]
    }
}
