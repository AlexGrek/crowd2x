//! The goal executors that exist: how each goal is actually pursued.
//!
//! One executor per goal, and one goal per need: eating, drinking, the toilet,
//! a go on the computer and a night in bed each have their own file even where their plans
//! look alike today, because the processes behind them will not stay alike.
//! What they genuinely share — where to stand to use something, how long to
//! wait for a gap — is here.

pub mod drink;
pub mod eat;
pub mod play;
pub mod relieve;
pub mod sleep;
pub mod wander;

pub use drink::DrinkGoal;
pub use eat::EatGoal;
pub use play::PlayGoal;
pub use relieve::RelieveGoal;
pub use sleep::SleepGoal;
pub use wander::WanderGoal;

use crate::map::Point;
use crate::sim::feature::FeatureKind;

use super::goal::GoalCtx;
use super::task::Task;

/// How long to stand aside for somebody who is in the way before trying the
/// same walk again. About as long as the old replan delay was, for the same
/// reason: long enough for a body to move, short enough that nobody looks
/// stuck.
///
/// **Watched seconds, and written as watched seconds** — the one duration in
/// the brain that is not about world time at all. What it waits for is another
/// *body* getting out of the way, and bodies move at the pace they are watched
/// moving at ([`crate::sim::clock`]); a minute of world here would be half a
/// step of somebody else's walk.
pub const WAIT_FOR_A_GAP: f32 = 0.5;

/// How many times in a row a walk blocked by a body is retried before the goal
/// gives up on it. The thing in the way is a body and bodies move — but not
/// always, and a unit frozen in a doorway is a wall for as long as it is
/// frozen.
pub const PATIENCE: u8 = 3;

/// How many times one visit may give up on a place and go for the next
/// nearest instead before the goal itself gives up ([`GoalProgress::Blocked`]).
/// Each reroute is a walk, and so a route; this is what keeps a unit in a
/// sealed room full of toilets from asking for one every other tick.
///
/// [`GoalProgress::Blocked`]: super::goal::GoalProgress::Blocked
pub const MAX_REROUTES: u8 = 3;

/// The feature of `kind` nearest this unit that it does not remember as out
/// of reach ([`OutOfReach`](super::memory::OutOfReach)). `None` when there
/// is none, not even one it gave up on: what a goal reroutes to straight
/// after giving up, which must never be the place it just gave up on.
///
/// Only among the features this unit may use: one in somebody else's
/// property is not there for it at all ([`Think::may_use`](crate::sim::Think::may_use)).
pub(super) fn nearest_in_reach(ctx: &GoalCtx<'_>, kind: FeatureKind) -> Option<Point> {
    let here = ctx.body.center_position();
    let now = ctx.think.clock.elapsed();
    let places = ctx.memory.out_of_reach();
    let key = ctx.body.home();
    ctx.think
        .features
        .nearest_where(kind, here, |cell| ctx.think.may_use(cell, key) && !places.contains(cell, now))
}

/// Where a visit *starts*: [`nearest_in_reach`], or — when it has given up on
/// every one of them lately — the one it gave up on longest ago, rather than
/// none. A lone toilet that was jammed is still the only toilet, and the
/// goal's own back-off ([`super::goal::Goals`]) already spaces the attempts
/// out; the memory is for choosing *between* places, not for refusing the
/// last one.
pub(super) fn choose_nearest(ctx: &GoalCtx<'_>, kind: FeatureKind) -> Option<Point> {
    nearest_in_reach(ctx, kind).or_else(|| {
        let now = ctx.think.clock.elapsed();
        let places = ctx.memory.out_of_reach();
        let here = ctx.body.center_position();
        let forgiven = |cell: Point| places.until(cell, now).unwrap_or(0.0);
        // Ties go to the nearer, then the lower cell, so the answer does not
        // depend on the order the memory was written in.
        let key = ctx.body.home();
        ctx.think.features.cells_of(kind).iter().copied().filter(|&cell| ctx.think.may_use(cell, key)).min_by(|&a, &b| {
            forgiven(a)
                .total_cmp(&forgiven(b))
                .then((here.manhattan_distance(a), a).cmp(&(here.manhattan_distance(b), b)))
        })
    })
}

/// Getting to a place to use it — a fridge, a toilet, a computer, a bed, any
/// feature a goal walks to — and what to do when the way there fails. One
/// of these lives in every goal that walks to a feature, so all of them
/// cope with a crowd the same way:
///
/// 1. **A body in the way** ([`GoalCtx::blocked_by`]): bodies move. Wait
///    [`WAIT_FOR_A_GAP`] and try the same place again, up to [`PATIENCE`]
///    times in a row.
/// 2. **No way there, or no end to the crowd**: give up on the place —
///    remember it as out of reach for a while
///    ([`OutOfReach`](super::memory::OutOfReach)) — and go for the next one
///    the goal offers, up to [`MAX_REROUTES`] times a visit.
/// 3. **Nowhere left**: the goal gives up on the visit (`Blocked`).
///
/// What it does not know is which places count or how they are used: the
/// goal says what "the next one" is ([`Seek::setback`]'s `elsewhere`) and
/// plans the walk itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Seek {
    /// The place, by cell.
    pub(super) target: Option<Point>,
    /// Setbacks caused by a body, in a row, at this place.
    retries: u8,
    /// Places given up on during this visit, each for another.
    reroutes: u8,
}

impl Seek {
    /// Already on the way to `place` — for a test that starts mid-visit.
    #[cfg(test)]
    pub(super) fn at(place: Point) -> Seek {
        Seek {
            target: Some(place),
            ..Seek::default()
        }
    }

    /// Head for `place`, a fresh choice.
    pub(super) fn choose(&mut self, place: Point) {
        self.target = Some(place);
        self.retries = 0;
    }

    /// Got somewhere, or picked the visit back up: patience starts again.
    pub(super) fn steady(&mut self) {
        self.retries = 0;
    }

    /// The visit is over, however it went.
    pub(super) fn forget(&mut self) {
        *self = Seek::default();
    }

    /// The walk to the target, or the step into it, failed. Decide what next,
    /// with the queue already cleared: `true` means [`Seek::target`] is where
    /// to go now — the same place after a wait (queued here), or the place
    /// `elsewhere` offered after giving up on this one — and the goal should
    /// plan the walk. `false` means give up on the visit.
    ///
    /// `elsewhere` is asked only after the target is remembered as out of
    /// reach, so [`nearest_in_reach`] never offers it straight back.
    pub(super) fn setback(
        &mut self,
        ctx: &mut GoalCtx<'_>,
        elsewhere: impl FnOnce(&GoalCtx<'_>) -> Option<Point>,
    ) -> bool {
        let Some(place) = self.target else {
            return false;
        };
        if ctx.blocked_by.is_some() && self.retries < PATIENCE {
            self.retries += 1;
            let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
            return true;
        }
        let now = ctx.think.clock.elapsed();
        ctx.memory.out_of_reach_mut().remember(place, now);
        ctx.think
            .log
            .push(format!("{} gave up on reaching {}, {}", ctx.body.uid(), place.x, place.y));
        if self.reroutes >= MAX_REROUTES {
            return false;
        }
        let Some(next) = elsewhere(ctx) else {
            return false;
        };
        self.reroutes += 1;
        self.choose(next);
        true
    }
}

/// Where to stand to use a feature.
pub(super) enum Stand {
    /// Already close enough.
    Here,
    At(Point),
    /// Nowhere passable touches it.
    Nowhere,
}

/// The cell to use the feature in `cell` from: beside it, passable, preferably
/// free, and nearest.
///
/// The four cells beside it and never its own — a prop blocks the cell it
/// stands in, so a fridge is used from the side or not at all. The crowd is
/// read for those four cells and nothing else: a human choosing the free side
/// of a fridge is a lookup, not a scan.
pub(super) fn stand_beside(ctx: &GoalCtx<'_>, cell: Point) -> Stand {
    let here = ctx.body.center_position();
    if here.manhattan_distance(cell) <= 1 {
        return Stand::Here;
    }
    let uid = ctx.body.uid();
    Point::CARDINALS
        .iter()
        .map(|&step| cell + step)
        .filter(|&beside| ctx.think.is_passable(beside))
        .min_by_key(|&beside| {
            (
                !ctx.think.occupancy.is_free_for(beside, uid),
                here.manhattan_distance(beside),
                beside,
            )
        })
        .map_or(Stand::Nowhere, Stand::At)
}
