//! Electricians walk to the nearest damaged box and repair it. Empty shifts
//! and unreachable repairs use the normal bounded goal backoff.

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::repair_box::REPAIR_SECONDS;
use super::{Seek, Stand, stand_beside};
use crate::save::{Invalid, pb};
use crate::sim::JobKind;

#[derive(Default)]
pub struct RepairGoal {
    seek: Seek,
}

impl RepairGoal {
    pub fn new() -> Self {
        Self::default()
    }

    fn plan(&self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(at) = self.seek.target else {
            return false;
        };
        if !ctx.think.electrical.is_damaged(at) {
            return false;
        }
        match stand_beside(ctx, at) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::repair_box(at, REPAIR_SECONDS));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Map, Point, Size, WALL};
    use crate::sim::job::WATCHES;
    use crate::sim::testing::{World, needy_human};
    use crate::sim::{GameEntity, Job};

    #[test]
    fn a_shift_without_damage_and_an_unreachable_fault_back_off_instead_of_routing_each_tick() {
        for unreachable in [false, true] {
            let at = Point::new(6, 3);
            let mut map = Map::new(Size::new(10, 7), FLOOR);
            if unreachable {
                for y in 0..7 {
                    map.set_terrain(Point::new(4, y), WALL);
                }
            }
            let mut world = World::new(map);
            if unreachable {
                world.facilities.electrical.damage(at);
            }
            let mut human = needy_human(Point::new(2, 3), 0.0, 0.0, 0.0);
            assert!(human.hire(Job::new(JobKind::Electrician, at, WATCHES[0])));
            crate::sim::walker::ROUTES_ASKED.with(|n| n.set(0));
            for _ in 0..300 {
                world.step(&mut human);
            }
            let routes = crate::sim::walker::ROUTES_ASKED.with(|n| n.get());
            assert!(routes < 30, "{routes} route searches in 300 ticks");
            if unreachable {
                assert!(human.brain().goals().cooldown(GoalId::Repair) > 0);
            } else {
                assert_ne!(human.brain().top_goal(), GoalId::Repair);
            }
        }
    }
}

impl GoalExecutor for RepairGoal {
    fn goal(&self) -> GoalId {
        GoalId::Repair
    }
    fn save(&self) -> pb::Executor {
        pb::Executor::Seek(self.seek.save())
    }
    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        self.seek.steady();
        if !self.plan(ctx) {
            self.seek.forget();
        }
    }
    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if !ctx.job.is_some_and(|job| job.kind == JobKind::Electrician) || ctx.biology.is_none() {
            return GoalProgress::Blocked;
        }
        if last == TaskResult::Success && matches!(ctx.finished, Some(Task::RepairBox(_))) {
            self.seek.forget();
            return GoalProgress::Achieved;
        }
        if last == TaskResult::Failed {
            ctx.tasks.clear();
            if matches!(ctx.finished, Some(Task::MoveTo(_)))
                && self.seek.setback(ctx, |_| None)
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
        let here = ctx.body.center_position();
        let now = ctx.think.clock.elapsed();
        let Some(at) = ctx
            .think
            .electrical
            .damaged()
            .iter()
            .copied()
            .filter(|&at| !ctx.memory.out_of_reach().contains(at, now))
            .min_by_key(|&at| (here.manhattan_distance(at), at))
        else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(at);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.seek.forget();
            GoalProgress::Blocked
        }
    }
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("box", format!("{:?}", self.seek.target))]
    }
}
