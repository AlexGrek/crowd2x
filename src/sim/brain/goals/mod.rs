//! The goal executors that exist: how each goal is actually pursued.
//!
//! One executor per goal, and one goal per need: eating, drinking and the
//! toilet each have their own file even where their plans look alike today,
//! because the processes behind them will not stay alike. What they genuinely
//! share — where to stand to use something, how long to wait for a gap — is
//! here.

pub mod drink;
pub mod eat;
pub mod relieve;
pub mod wander;

pub use drink::DrinkGoal;
pub use eat::EatGoal;
pub use relieve::RelieveGoal;
pub use wander::WanderGoal;

use crate::map::Point;

use super::goal::GoalCtx;

/// How long to stand aside for somebody who is in the way before trying the
/// same walk again, in seconds. About as long as the old replan delay was,
/// for the same reason: long enough for a body to move, short enough that
/// nobody looks stuck.
pub const WAIT_FOR_A_GAP: f32 = 0.5;

/// How many times in a row a walk blocked by a body is retried before the goal
/// gives up on it. The thing in the way is a body and bodies move — but not
/// always, and a unit frozen in a doorway is a wall for as long as it is
/// frozen.
pub const PATIENCE: u8 = 3;

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
/// Four cells and the feature's own — props do not block terrain, so standing
/// in the feature's cell is allowed as a last resort. The crowd is read for
/// those five cells and nothing else: a human choosing the free side of a
/// fridge is a lookup, not a scan.
pub(super) fn stand_beside(ctx: &GoalCtx<'_>, cell: Point) -> Stand {
    let here = ctx.body.center_position();
    if here.manhattan_distance(cell) <= 1 {
        return Stand::Here;
    }
    let uid = ctx.body.uid();
    Point::CARDINALS
        .iter()
        .map(|&step| cell + step)
        .chain(std::iter::once(cell))
        .filter(|&beside| ctx.think.is_passable(beside))
        .min_by_key(|&beside| {
            (
                !ctx.think.occupancy.is_free_for(beside, uid),
                beside == cell,
                here.manhattan_distance(beside),
                beside,
            )
        })
        .map_or(Stand::Nowhere, Stand::At)
}
