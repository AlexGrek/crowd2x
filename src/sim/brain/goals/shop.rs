//! [`ShopGoal`]: keep the store — a shopkeeper's shift.
//!
//! **A shopkeeper always checks the superfridge.** A round is a look in it
//! ([`CheckStock`](crate::sim::brain::tasks::CheckStock): how full it is,
//! and the courier dispatch called when it is down to the call level), then
//! a stretch minding the shop at its own counter
//! ([`Work`](crate::sim::brain::tasks::Work), [`MIND_SECONDS`]); `Achieved`
//! after each round, and the routine starts the next while the shift is due.
//! So the superfridge is looked over every quarter of an hour of a shift, and
//! with four shopkeepers on the four quarters of the day ([`SHOP_SHIFTS`]),
//! round the clock.
//!
//! Which superfridge: the one nearest its counter — a store has one. A
//! counter with none near is only minded. What is going on is kept the way
//! [`WorkGoal`](super::WorkGoal) keeps it: the place it is going to, in a
//! [`Seek`] — the superfridge for a look, the counter for a stretch.
//!
//! [`SHOP_SHIFTS`]: crate::sim::job::SHOP_SHIFTS

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::CHECK_SECONDS;
use super::{stand_beside, Seek, Stand};

/// A stretch at the counter between two looks in the superfridge: a quarter
/// of an hour of world.
pub const MIND_SECONDS: f32 = watched(15.0 * MINUTE);

/// Keep the store: look in the superfridge, mind the shop, and again.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ShopGoal {
    seek: Seek,
}

impl ShopGoal {
    pub fn new() -> ShopGoal {
        ShopGoal::default()
    }

    /// Its own counter, if it is one: the job's workplace.
    fn counter(ctx: &GoalCtx<'_>) -> Option<Point> {
        let at = ctx.job?.workplace;
        ctx.think.features.has(FeatureKind::ShopCounter, at).then_some(at)
    }

    /// The superfridge it keeps an eye on: the one nearest its counter.
    fn fridge(ctx: &GoalCtx<'_>, counter: Point) -> Option<Point> {
        ctx.think.features.nearest(FeatureKind::Superfridge, counter)
    }

    /// Queue the rest from what is true now: the walk beside the target
    /// (unless already there), and the look or the stretch there. `false`
    /// when there is nowhere to stand.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let (Some(target), Some(counter)) = (self.seek.target, Self::counter(ctx)) else {
            return false;
        };
        match stand_beside(ctx, target) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let task = if target == counter {
            Task::work_at(FeatureKind::ShopCounter, counter, MIND_SECONDS)
        } else {
            Task::check_stock(target, CHECK_SECONDS)
        };
        let _ = ctx.tasks.push_back(task);
        true
    }

    /// Head for `place` and queue what is done there; `Blocked` with
    /// nowhere to stand.
    fn go(&mut self, ctx: &mut GoalCtx<'_>, place: Point) -> GoalProgress {
        self.seek.choose(place);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.seek.forget();
            GoalProgress::Blocked
        }
    }
}

impl GoalExecutor for ShopGoal {
    fn goal(&self) -> GoalId {
        GoalId::Shopkeep
    }

    fn save(&self) -> pb::Executor {
        let ShopGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: on its way to its own counter or the superfridge by
    /// it, it goes on there; anywhere else — let go and hired at another
    /// store — it starts the round again.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        let Some(target) = self.seek.target else {
            return;
        };
        let counter = Self::counter(ctx);
        let fridge = counter.and_then(|counter| Self::fridge(ctx, counter));
        if Some(target) != counter && Some(target) != fridge {
            self.seek.forget();
            return;
        }
        self.seek.steady();
        if !self.plan(ctx) {
            self.seek.forget();
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() {
            return GoalProgress::Blocked;
        }
        let Some(counter) = Self::counter(ctx) else {
            return GoalProgress::Blocked;
        };

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // round is given up and the hold spaces out the next.
                if self.seek.setback(ctx, |_| None) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Pulled away: worked out again from where it stands.
                ctx.tasks.clear();
                self.seek.forget();
                return GoalProgress::Working;
            }
            // Looked in: now the counter.
            (TaskResult::Success, Some(Task::CheckStock(_))) => {
                ctx.tasks.clear();
                return self.go(ctx, counter);
            }
            // A stretch minded: one round done.
            (TaskResult::Success, Some(Task::Work(_))) => {
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        // A round starts with a look in the superfridge, if there is one.
        let first = Self::fridge(ctx, counter).unwrap_or(counter);
        self.go(ctx, first)
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "going to",
            match self.seek.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "none".to_string(),
            },
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR, WALL};
    use crate::sim::job::{Job, JobKind, SHOP_SHIFTS};
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::{GameEntity, ProcessId};

    fn keeper() -> crate::sim::Human {
        let mut human = needy_human(Point::new(2, 2), 0.0, 0.0, 0.0);
        for process in ProcessId::ALL { human.biology_mut().unwrap().set_running(process, false); }
        human.hire(Job::new(JobKind::Shopkeeper, Point::new(8, 3), SHOP_SHIFTS[0]));
        human
    }

    #[test]
    fn a_counter_without_a_superfridge_is_still_minded() {
        let mut map = Map::new(Size::new(12, 10), FLOOR);
        prop_at(&mut map, "shop counter", Point::new(8, 3));
        let mut world = World::new(map);
        let mut human = keeper();
        let mut minded = false;
        for _ in 0..1000 {
            world.step(&mut human);
            minded |= matches!(human.brain().current_task(), Some(Task::Work(_)));
        }
        assert!(minded);
        assert!(world.dispatch.orders().is_empty());
    }

    #[test]
    fn an_unreachable_superfridge_does_not_start_a_route_every_tick() {
        let mut map = Map::new(Size::new(12, 10), FLOOR);
        prop_at(&mut map, "shop counter", Point::new(8, 3));
        let fridge = Point::new(8, 7);
        prop_at(&mut map, "superfridge", fridge);
        for cell in fridge.cardinal_neighbours() { map.set_terrain(cell, WALL); }
        let mut world = World::new(map);
        let mut human = keeper();
        let before = crate::sim::walker::ROUTES_ASKED.get();
        for _ in 0..2000 { world.step(&mut human); }
        assert!(crate::sim::walker::ROUTES_ASKED.get() - before < 30);
        assert!(world.dispatch.orders().is_empty());
    }
}
