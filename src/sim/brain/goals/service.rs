//! A worn robot's trip in for an overhaul: to the robot factory's nearest
//! assembly bay, and waiting beside it, a stretch at a time, until an engineer
//! has overhauled it and `RobotRoutine` lets go (`sim::robots`). While it
//! waits there it says so every tick (`Effect::AwaitService`, asked by the
//! robot itself), which is how the engineers know.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{nearest_in_reach, stand_beside, Seek, Stand};

/// One stretch of waiting at the bay.
pub const SERVICE_WAIT: f32 = watched(15.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ServiceGoal {
    seek: Seek,
}

impl ServiceGoal {
    pub fn new() -> ServiceGoal {
        ServiceGoal::default()
    }

    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(bay) = self.seek.target else {
            return false;
        };
        match stand_beside(ctx, bay) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::wait(SERVICE_WAIT));
        true
    }
}

/// Whether `cell` is beside an assembly bay, and which.
pub fn bay_beside(features: &crate::sim::Features, cell: Point) -> Option<Point> {
    features.nearest_where(FeatureKind::RepairBay, cell, |_| true).filter(|bay| bay.manhattan_distance(cell) <= 1)
}

impl GoalExecutor for ServiceGoal {
    fn goal(&self) -> GoalId {
        GoalId::Service
    }

    fn save(&self) -> pb::Executor {
        pb::Executor::Seek(self.seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.seek.steady();
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if last == TaskResult::Failed {
            ctx.tasks.clear();
            if matches!(ctx.finished, Some(Task::MoveTo(_)))
                && self.seek.setback(ctx, |ctx| nearest_in_reach(ctx, FeatureKind::RepairBay))
                && self.plan(ctx)
            {
                return GoalProgress::Working;
            }
            self.seek.forget();
            return GoalProgress::Blocked;
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        if self.seek.target.is_none() {
            let Some(bay) = nearest_in_reach(ctx, FeatureKind::RepairBay) else {
                return GoalProgress::Blocked;
            };
            self.seek.choose(bay);
        }
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.seek.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("bay", self.seek.target.map_or("none".to_string(), |bay| format!("{}, {}", bay.x, bay.y)))]
    }
}
