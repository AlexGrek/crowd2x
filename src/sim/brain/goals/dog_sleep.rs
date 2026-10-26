//! Pet sleep: settle on free ground near the owner while the owner is asleep.

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::follow::CLOSE;
use super::{PATIENCE, WAIT_FOR_A_GAP};
use crate::map::Point;
use crate::save::{Invalid, invalid, pb};
use crate::sim::clock::{HOUR, watched};

#[derive(Default)]
pub struct DogSleepGoal {
    retries: u8,
}

impl GoalExecutor for DogSleepGoal {
    fn goal(&self) -> GoalId {
        GoalId::Sleep
    }

    fn save(&self) -> pb::Executor {
        pb::Executor::Wander(pb::Wander {
            target: None,
            retries: self.retries as u32,
        })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Wander(saved) = saved else {
            return invalid("dog sleep keeps its approach retries");
        };
        self.retries =
            u8::try_from(saved.retries).map_err(|_| Invalid::new("dog sleep: too many retries"))?;
        Ok(())
    }

    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.retries = 0;
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let Some(owner) = ctx
            .think
            .bonds
            .partner(ctx.body.uid())
            .filter(|owner| owner.asleep)
        else {
            return GoalProgress::Blocked;
        };
        if last == TaskResult::Failed {
            ctx.tasks.clear();
            if ctx.blocked_by.is_some() && self.retries < PATIENCE {
                self.retries += 1;
                let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                return GoalProgress::Working;
            }
            return GoalProgress::Blocked;
        }
        if last == TaskResult::Success && matches!(ctx.finished, Some(Task::MoveTo(_))) {
            self.retries = 0;
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        let here = ctx.body.center_position();
        let suitable = |cell: Point| {
            cell != owner.cell
                && ctx.think.is_passable(cell)
                && !ctx.think.doors.is_door(cell)
                && (cell == here || !ctx.think.occupancy.is_occupied(cell))
                && ctx.think.map.navigation().may_reach(here, cell)
                && ctx.think.may_use(cell, ctx.body.home())
        };
        // Already close: stay put. Otherwise inspect just the small square
        // around the owner, never the crowd or a route inside a goal.
        let spot = if here.chebyshev_distance(owner.cell) <= CLOSE && suitable(here) {
            Some(here)
        } else {
            (-CLOSE..=CLOSE)
                .flat_map(|y| (-CLOSE..=CLOSE).map(move |x| owner.cell.offset(x, y)))
                .filter(|&cell| suitable(cell))
                .min_by_key(|&cell| (here.manhattan_distance(cell), cell.y, cell.x))
        };
        let Some(spot) = spot else {
            return GoalProgress::Blocked;
        };
        if spot != here {
            let _ = ctx.tasks.push_back(Task::move_to(spot));
        }
        let _ = ctx.tasks.push_back(Task::dog_sleep(spot, watched(HOUR)));
        GoalProgress::Working
    }
}
