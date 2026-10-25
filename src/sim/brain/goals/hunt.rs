//! [`HuntGoal`]: a deathdog runs down the person it spotted, and bites.
//!
//! Raised by `HuntRoutine` while the deathdog saw somebody outside the site
//! lately (`brain::alarm`, [`Memory::spotted`](super::super::memory::Memory::spotted)).
//! The plan is a loop of two tasks: [`Chase`](super::super::tasks::Chase)
//! until in reach, [`Bite`](super::super::tasks::Bite), and chase again —
//! the second chase told to wait out [`BITE_INTERVAL`] before it may end in
//! another bite, so the wait travels in the task and this goal keeps no state
//! of its own.
//!
//! A chase that fails with nobody in the way means the quarry got away — out
//! of sight round it, or onto the site, where no deathdog follows: forgotten,
//! and [`GoalProgress::Achieved`]. One that a body stopped is tried again
//! after a moment ([`WAIT_FOR_A_GAP`]).

use crate::save::{invalid, pb, Invalid};
use crate::sim::deathdogs::BITE_INTERVAL;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::memory::Spotted;
use super::super::task::{Task, TaskResult};
use super::WAIT_FOR_A_GAP;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HuntGoal;

impl HuntGoal {
    pub fn new() -> HuntGoal {
        HuntGoal
    }
}

/// How many ticks of `dt` watched seconds [`BITE_INTERVAL`] is.
fn bite_interval_ticks(dt: f32) -> u16 {
    if dt > 0.0 { (BITE_INTERVAL / dt).ceil().min(u16::MAX as f32) as u16 } else { 0 }
}

impl GoalExecutor for HuntGoal {
    fn goal(&self) -> GoalId {
        GoalId::Hunt
    }

    /// Nothing of its own: saved as an empty wander, which is what it
    /// restores from.
    fn save(&self) -> pb::Executor {
        pb::Executor::Wander(pb::Wander { target: None, retries: 0 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        match saved {
            pb::Executor::Wander(pb::Wander { target: None, retries: 0 }) => Ok(()),
            _ => invalid("hunting keeps nothing"),
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let Some(spotted) = ctx.memory.spotted() else {
            return GoalProgress::Achieved;
        };
        let again = |rest: u16| Task::chase(spotted.who, spotted.at, rest);
        match (last, ctx.finished) {
            // Caught up: where it is now is where to bite.
            (TaskResult::Success, Some(Task::Chase(chase))) => {
                ctx.memory.spot(Spotted { at: chase.at, when: ctx.think.clock.elapsed(), ..spotted });
                let _ = ctx.tasks.push_back(Task::bite(chase.at));
            }
            // Bitten, or slipped out of reach as the teeth came down: after it
            // again — a wait first, if it bit.
            (TaskResult::Success, Some(Task::Bite(_))) => {
                let _ = ctx.tasks.push_back(again(bite_interval_ticks(ctx.think.dt)));
            }
            (TaskResult::Failed, Some(Task::Bite(_))) => {
                let _ = ctx.tasks.push_back(again(0));
            }
            // Somebody in the way: a moment, then after it again.
            (TaskResult::Failed, Some(Task::Chase(_))) if ctx.blocked_by.is_some() => {
                ctx.tasks.clear();
                let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                let _ = ctx.tasks.push_back(again(0));
            }
            // Lost it, or it is safe inside.
            (TaskResult::Failed, Some(Task::Chase(_))) => {
                ctx.memory.forget_spotted();
                return GoalProgress::Achieved;
            }
            _ => {
                if ctx.tasks.is_empty() {
                    let _ = ctx.tasks.push_back(again(0));
                }
            }
        }
        GoalProgress::Working
    }
}
