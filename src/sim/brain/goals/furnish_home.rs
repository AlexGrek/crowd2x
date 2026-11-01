//! Shopping and bringing home one vase, resumed from the world's receipt.
use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{Seek, Stand, nearest_in_reach, stand_beside};
use crate::save::{Invalid, pb};
use crate::sim::clock::{MINUTE, watched};
use crate::sim::{FeatureKind, ItemKind};

#[derive(Default)]
pub struct FurnishHomeGoal {
    seek: Seek,
}
impl FurnishHomeGoal {
    pub fn new() -> Self {
        Self::default()
    }
}
impl GoalExecutor for FurnishHomeGoal {
    fn goal(&self) -> GoalId {
        GoalId::FurnishHome
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
        let uid = ctx.body.uid();
        let receipt = ctx.think.decor.receipt(uid);
        if receipt.is_some() || matches!(ctx.finished, Some(Task::BuyVase(_))) {
            ctx.memory.decor_day = ctx.think.clock.day();
        }
        if matches!(ctx.finished, Some(Task::PlaceVase(_))) && receipt.is_none() {
            self.seek.forget();
            return GoalProgress::Achieved;
        }
        if matches!(ctx.finished, Some(Task::BuyVase(_))) && receipt.is_none() {
            self.seek.forget();
            return GoalProgress::Blocked;
        }
        if last == TaskResult::Failed {
            ctx.tasks.clear();
            if !self
                .seek
                .setback(ctx, |ctx| nearest_in_reach(ctx, FeatureKind::DecorSection))
            {
                ctx.memory.decor_day = ctx.think.clock.day();
                self.seek.forget();
                return GoalProgress::Blocked;
            }
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        let (at, task) = if let Some(r) = receipt {
            if ctx.body.home() != Some(r.home)
                || !ctx
                    .inventory
                    .is_some_and(|p| p.count(ItemKind::FlowerVase) > 0)
            {
                return GoalProgress::Blocked;
            }
            if ctx.think.occupancy.occupant(r.cell) == Some(uid) {
                self.seek.choose(r.cell);
                let _ = ctx.tasks.push_back(Task::move_beside(r.cell));
                return GoalProgress::Working;
            }
            if ctx.think.occupancy.occupant(r.cell).is_some()
                || ctx.think.corpses.at(r.cell).is_some()
            {
                return if self.seek.wait_again(ctx) {
                    GoalProgress::Working
                } else {
                    GoalProgress::Blocked
                };
            }
            (r.cell, Task::place_vase(r.cell, watched(5.0 * MINUTE)))
        } else {
            let Some(at) = self
                .seek
                .target
                .or_else(|| nearest_in_reach(ctx, FeatureKind::DecorSection))
            else {
                ctx.memory.decor_day = ctx.think.clock.day();
                return GoalProgress::Blocked;
            };
            // At most one new shopping attempt per world day; a paid receipt always resumes.
            (at, Task::buy_vase(at, watched(5.0 * MINUTE)))
        };
        if self.seek.target != Some(at) {
            self.seek.choose(at);
        }
        match stand_beside(ctx, at) {
            Stand::Nowhere => {
                ctx.memory.decor_day = ctx.think.clock.day();
                return GoalProgress::Blocked;
            }
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(task);
        GoalProgress::Working
    }
}
