//! An engineer's shift at the robot factory (`sim::robots`), from what is true
//! now, one thing at a time, each `Achieved` once done:
//!
//! 1. **a worn robot waiting at the bay** — the nearest — overhauled, from
//!    beside it (`Task::overhaul`);
//! 2. **a robot to build** (one under way, or one due with room for it,
//!    `Works::due`): steel carried goes onto the factory's parts rack; with the
//!    robot's steel on the rack, or the build under way, an hour's work at the
//!    assembly bay (`Task::assemble`); short of steel, a packful from the
//!    warehouse's steel rack;
//! 3. steel carried with nothing to build goes onto the parts rack all the
//!    same, ready for the next;
//! 4. otherwise an hour tinkering at its own workbench (`Task::work_at`).

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;
use crate::sim::job::JobKind;
use crate::sim::robots::ROBOT_STEEL;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::{ASSEMBLE_SECONDS, DEPOSIT_SECONDS, OVERHAUL_SECONDS, WITHDRAW_SECONDS};
use super::work::WORK_SECONDS;
use super::{nearest_store, stand_beside, Seek, Stand};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EngineerGoal {
    seek: Seek,
}

impl EngineerGoal {
    pub fn new() -> EngineerGoal {
        EngineerGoal::default()
    }

    /// Head for `place`, queue the walk beside it and `work` there. `false`
    /// when nowhere beside it can be stood on.
    fn go(&mut self, ctx: &mut GoalCtx<'_>, place: Point, work: Task) -> bool {
        self.seek.choose(place);
        match stand_beside(ctx, place) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(work);
        true
    }

    /// The parts rack holding the most steel, or the nearest: the robot
    /// factory's.
    fn parts_rack(ctx: &GoalCtx<'_>) -> Option<Point> {
        nearest_store(ctx, FeatureKind::PartsRack, false)
    }

    /// Whether the robot's steel is on a parts rack.
    fn steel_in(ctx: &GoalCtx<'_>) -> bool {
        ctx.think.features.cells_of(FeatureKind::PartsRack).iter().any(|&rack| ctx.think.bins.stored(rack) >= ROBOT_STEEL)
    }

    /// What to do next, and the walk to it; `Blocked` with nothing it can do.
    fn next(&mut self, ctx: &mut GoalCtx<'_>) -> GoalProgress {
        let here = ctx.body.center_position();
        let now = ctx.think.clock.elapsed();
        // Looked up front: the borrow of its memory ends before it plans.
        let (waiting, bay) = {
            let places = ctx.memory.out_of_reach();
            let waiting = ctx.think.robots.waiting().iter().map(|&(_, cell)| cell).filter(|&cell| !places.contains(cell, now)).min_by_key(|&cell| (here.manhattan_distance(cell), cell));
            let bay = ctx.think.features.nearest_where(FeatureKind::RepairBay, here, |cell| !places.contains(cell, now));
            (waiting, bay)
        };
        // 1. A robot waiting for an overhaul.
        if let Some(cell) = waiting
            && self.go(ctx, cell, Task::overhaul(cell, OVERHAUL_SECONDS))
        {
            return GoalProgress::Working;
        }
        let robots = ctx.think.robots;
        let due = robots.works().due(now, robots.population());
        let carrying = ctx.inventory.is_some_and(|pack| pack.count(ItemKind::Steel) > 0);
        // 2, 3. Steel carried goes onto the parts rack.
        if carrying
            && let Some(rack) = Self::parts_rack(ctx)
            && self.go(ctx, rack, Task::deposit(rack, DEPOSIT_SECONDS))
        {
            return GoalProgress::Working;
        }
        if due {
            if let Some(bay) = bay
                && (robots.works().is_started() || Self::steel_in(ctx))
                && self.go(ctx, bay, Task::assemble(bay, ASSEMBLE_SECONDS))
            {
                return GoalProgress::Working;
            }
            if !robots.works().is_started()
                && Self::parts_rack(ctx).is_some()
                && let Some(rack) = nearest_store(ctx, FeatureKind::SteelRack, true)
                && self.go(ctx, rack, Task::withdraw(rack, WITHDRAW_SECONDS))
            {
                return GoalProgress::Working;
            }
        }
        // 4. Its own workbench.
        let Some(job) = ctx.job else {
            return GoalProgress::Blocked;
        };
        let bench = job.workplace;
        if !ctx.think.features.has(FeatureKind::RobotWorkbench, bench) {
            return GoalProgress::Blocked;
        }
        if self.go(ctx, bench, Task::work_at(FeatureKind::RobotWorkbench, bench, WORK_SECONDS)) {
            GoalProgress::Working
        } else {
            GoalProgress::Blocked
        }
    }
}

impl GoalExecutor for EngineerGoal {
    fn goal(&self) -> GoalId {
        GoalId::Engineer
    }

    fn save(&self) -> pb::Executor {
        pb::Executor::Seek(self.seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Plans from what is true now, every time: nothing kept but the place
    /// last headed for, which `next` chooses again.
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.seek.forget();
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if !ctx.job.is_some_and(|job| job.kind == JobKind::Engineer) || ctx.biology.is_none() {
            return GoalProgress::Blocked;
        }
        match (last, ctx.finished) {
            (TaskResult::Success, Some(Task::Work(task))) => {
                let uid = ctx.body.uid();
                ctx.think.log.unit(uid, format!("{uid} tinkered an hour at the robot workbench at {}, {}", task.desk.x, task.desk.y));
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::Overhaul(_) | Task::Assemble(_) | Task::Deposit(_))) => {
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait for a gap. No way there: remembered
                // out of reach, and the next thing chosen afresh.
                // After the wait, `next` chooses again.
                if ctx.blocked_by.is_some() && self.seek.wait_again(ctx) {
                    return GoalProgress::Working;
                }
                let _ = self.seek.setback(ctx, |_| None);
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            // The robot went, a build's steel was taken, a rack emptied: the
            // world changed under it. Choose again.
            (TaskResult::Failed, _) => {
                ctx.tasks.clear();
                self.seek.forget();
            }
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        self.next(ctx)
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("heading for", self.seek.target.map_or("nothing".to_string(), |at| format!("{}, {}", at.x, at.y)))]
    }
}
