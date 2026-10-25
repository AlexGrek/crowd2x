//! [`FleeGoal`]: run for the nearest way in, and shelter inside until the
//! fright is over.
//!
//! Raised by `FleeRoutine` over everything else while something scared the
//! unit lately — a deathdog seen, somebody seen running from one, a bite
//! (`brain::alarm`, `sim::wounds`). **Inside the site is safe**
//! (`sim::deathdogs`), so the plan is short:
//!
//! * outside, or on the very edge of the site ([`SAFE_DEPTH`]): run to the
//!   refuge behind the nearest way in ([`WaysIn::nearest`](crate::sim::refuge::WaysIn::nearest)) —
//!   flat out, because the brain tells the walker it is panicking while this
//!   is in charge (`Walker::panic`);
//! * deep enough inside: stand where it is ([`SHELTER`] at a time), so as not
//!   to walk straight back out to whatever is waiting at the gate.
//!
//! Somebody in the way is waited for and the run tried again, as a wanderer
//! does ([`PATIENCE`]); no way in at all is [`GoalProgress::Blocked`].

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::refuge::{depth, SAFE_DEPTH};

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{PATIENCE, WAIT_FOR_A_GAP};

/// How long one stretch of sheltering is, in watched seconds: five minutes
/// of world, after which it looks at where it stands again.
pub const SHELTER: f32 = watched(5.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FleeGoal {
    /// The refuge it is running for.
    target: Option<Point>,
    /// Runs to `target` blocked by a body so far, in a row.
    retries: u8,
}

impl FleeGoal {
    pub fn new() -> FleeGoal {
        FleeGoal::default()
    }

    fn forget(&mut self) {
        self.target = None;
        self.retries = 0;
    }
}

impl GoalExecutor for FleeGoal {
    fn goal(&self) -> GoalId {
        GoalId::Flee
    }

    fn save(&self) -> pb::Executor {
        let FleeGoal { target, retries } = *self;
        pb::Executor::Wander(pb::Wander { target: target.map(Into::into), retries: retries as u32 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Wander(flight) = saved else {
            return invalid("fleeing is a refuge and its retries");
        };
        let Ok(retries) = u8::try_from(flight.retries) else {
            return invalid(format!("flee: {} retries", flight.retries));
        };
        self.target = flight.target.map(Into::into);
        self.retries = retries;
        Ok(())
    }

    /// Something scared it: say so, once a fright, and plan from where it
    /// stands — a refuge picked before is picked again if it is still the
    /// nearest.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        self.forget();
        let uid = ctx.body.uid();
        ctx.think.log.unit(uid, format!("{uid} panicked"));
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        match last {
            TaskResult::Success if !ctx.tasks.is_empty() => return GoalProgress::Working,
            TaskResult::Failed => {
                ctx.tasks.clear();
                match (self.target, ctx.blocked_by) {
                    (Some(target), Some(_)) if self.retries < PATIENCE => {
                        self.retries += 1;
                        let _ = ctx.tasks.push_front(Task::wait(WAIT_FOR_A_GAP));
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
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        let here = ctx.body.center_position();
        if depth(ctx.think.map, here) >= SAFE_DEPTH || self.target == Some(here) {
            let _ = ctx.tasks.push_back(Task::wait(SHELTER));
            return GoalProgress::Working;
        }
        let Some(refuge) = ctx.think.ways_in.nearest(here) else {
            return GoalProgress::Blocked;
        };
        if refuge == here {
            self.target = Some(refuge);
            let _ = ctx.tasks.push_back(Task::wait(SHELTER));
            return GoalProgress::Working;
        }
        if self.target != Some(refuge) {
            self.target = Some(refuge);
            self.retries = 0;
        }
        let _ = ctx.tasks.push_back(Task::move_to(refuge));
        GoalProgress::Working
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "refuge",
            match self.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "none".to_string(),
            },
        )]
    }
}
