//! Return to safe ground in response to a spoken guard command.
use crate::map::Point;
use crate::save::{Invalid, invalid, pb};
use crate::sim::refuge::{SAFE_DEPTH, depth};

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{PATIENCE, WAIT_FOR_A_GAP};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ReturnInsideGoal {
    /// The refuge it is running for.
    target: Option<Point>,
    /// Runs to `target` blocked by a body so far, in a row.
    retries: u8,
}

impl ReturnInsideGoal {
    pub fn new() -> ReturnInsideGoal {
        ReturnInsideGoal::default()
    }

    fn forget(&mut self) {
        self.target = None;
        self.retries = 0;
    }
}

impl GoalExecutor for ReturnInsideGoal {
    fn goal(&self) -> GoalId {
        GoalId::ReturnInside
    }

    fn save(&self) -> pb::Executor {
        let ReturnInsideGoal { target, retries } = *self;
        pb::Executor::Wander(pb::Wander {
            target: target.map(Into::into),
            retries: retries as u32,
        })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Wander(flight) = saved else {
            return invalid("returning inside is a refuge and its retries");
        };
        let Ok(retries) = u8::try_from(flight.retries) else {
            return invalid(format!("return inside: {} retries", flight.retries));
        };
        self.target = flight.target.map(Into::into);
        self.retries = retries;
        Ok(())
    }

    /// Rebuild the route from wherever the order was heard.
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.forget();
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
            ctx.memory.returning_inside = false;
            return GoalProgress::Achieved;
        }
        let Some(refuge) = ctx.think.ways_in.nearest(here) else {
            return GoalProgress::Blocked;
        };
        if refuge == here {
            self.target = Some(refuge);
            ctx.memory.returning_inside = false;
            return GoalProgress::Achieved;
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
