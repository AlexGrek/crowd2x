//! The goal executors that exist: how each goal is actually pursued.

pub mod eat;
pub mod wander;

pub use eat::EatGoal;
pub use wander::WanderGoal;

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
