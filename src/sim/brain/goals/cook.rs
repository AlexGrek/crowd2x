//! A food factory worker's shift: fetch wheat from a hopper, cook one meal
//! at its own powered cooker, and put the meal on a pallet for a courier.
//! Plans from current stock after every handover; Seek handles blocked
//! walks with the same retry and unreachable memory as the steel works.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::{ItemKind, WHEAT_PER_MEAL};

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{nearest_store, stand_beside, Seek, Stand};
use crate::sim::brain::tasks::{COOK_SECONDS, DEPOSIT_SECONDS, WITHDRAW_SECONDS};

/// Cook meals for a living.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CookGoal {
    seek: Seek,
}

impl CookGoal {
    pub fn new() -> CookGoal {
        CookGoal::default()
    }

    /// The food factory worker's own cooker: the job's workplace, and
    /// a cooker there with power.
    fn cooker(ctx: &GoalCtx<'_>) -> Option<Point> {
        let at = ctx.job?.workplace;
        ctx.think.features.has(FeatureKind::FoodCooker, at).then_some(at)
    }

    /// The work to do once beside `target`: cook at the cooker, store
    /// meals on a pallet, fetch wheat from a hopper.
    fn work_at(ctx: &GoalCtx<'_>, target: Point) -> Task {
        if ctx.think.features.has(FeatureKind::MealPallet, target) {
            Task::deposit(target, DEPOSIT_SECONDS)
        } else if ctx.think.features.has(FeatureKind::WheatHopper, target) {
            Task::withdraw(target, WITHDRAW_SECONDS)
        } else {
            Task::cook(target, COOK_SECONDS)
        }
    }

    /// Head for `place` (unless already on the way), queue the walk beside
    /// it (unless already there) and the work there. `false` when nowhere
    /// beside it can be stood on.
    fn go(&mut self, ctx: &mut GoalCtx<'_>, place: Point) -> bool {
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
        self.plan(ctx)
    }

    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(target) = self.seek.target else {
            return false;
        };
        match stand_beside(ctx, target) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Self::work_at(ctx, target));
        true
    }

    /// Whether `target` is still somewhere to go: its own cooker working, a
    /// pallet, or a hopper with wheat in it.
    fn still_worth_it(ctx: &GoalCtx<'_>, target: Point) -> bool {
        match ctx.job {
            Some(job) if job.workplace == target => Self::cooker(ctx).is_some(),
            _ => {
                ctx.think.features.has(FeatureKind::MealPallet, target)
                    || (ctx.think.features.has(FeatureKind::WheatHopper, target) && ctx.think.bins.stored(target) > 0)
            }
        }
    }
}

impl GoalExecutor for CookGoal {
    fn goal(&self) -> GoalId {
        GoalId::Cook
    }

    fn save(&self) -> pb::Executor {
        let CookGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: a target no longer worth going to — a hopper emptied
    /// meanwhile, a cooker that is not the job's any more — is forgotten;
    /// [`CookGoal::process`] plans the rest from where it stands.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        match self.seek.target {
            Some(target) if Self::still_worth_it(ctx, target) => self.seek.steady(),
            Some(_) => self.seek.forget(),
            None => {}
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(job), Some(inventory)) = (ctx.biology, ctx.job, ctx.inventory) else {
            return GoalProgress::Blocked;
        };
        let cooker = job.workplace;

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // next nearest of the same — but no other cooker is its own.
                let retry = match self.seek.target {
                    Some(target) if target == cooker => self.seek.setback(ctx, |_| None),
                    Some(target) if ctx.think.features.has(FeatureKind::MealPallet, target) => {
                        self.seek.setback(ctx, |ctx| nearest_store(ctx, FeatureKind::MealPallet, false))
                    }
                    _ => self.seek.setback(ctx, |ctx| nearest_store(ctx, FeatureKind::WheatHopper, true)),
                };
                if retry && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            // The hopper emptied under it, the cooker lost its power, it was
            // pulled away: worked out again below, from what is true now.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Deposit(_))) => {
                // Meals on the pallet: one piece of work done.
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        let (wheat, meals) = (inventory.count(ItemKind::Wheat), inventory.count(ItemKind::Meal));
        let next = if meals > 0 {
            nearest_store(ctx, FeatureKind::MealPallet, false)
        } else if wheat >= WHEAT_PER_MEAL {
            Self::cooker(ctx)
        } else {
            let hopper = nearest_store(ctx, FeatureKind::WheatHopper, true);
            if hopper.is_none() {
                // Nothing to cook: held off, and the hold doubling is what
                // spaces these lines out.
                ctx.think.log.unit(ctx.body.uid(), format!("{} found no wheat to cook", ctx.body.uid()));
            }
            hopper
        };
        match next {
            Some(place) if self.go(ctx, place) => GoalProgress::Working,
            _ => {
                self.seek.forget();
                GoalProgress::Blocked
            }
        }
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
    use crate::sim::clock::{Clock, HOUR, TIME_SCALE};
    use crate::sim::job::{Job, JobKind, OFFICE_HOURS};
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::{GameEntity, Human, ProcessId};

    const COOKER: Point = Point::new(8, 5);
    const HOPPER: Point = Point::new(2, 5);
    const PALLET: Point = Point::new(8, 8);

    fn setup(wheat: u32) -> (World, Human) {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for (name, cell) in [("food cooker", COOKER), ("wheat hopper", HOPPER), ("meal pallet", PALLET)] {
            prop_at(&mut map, name, cell);
        }
        assert!(map.set_initial_stock(HOPPER, wheat));
        let mut world = World::new(map);
        world.clock = Clock::after_watching(HOUR as f64 / TIME_SCALE as f64);
        let mut human = needy_human(Point::new(5, 5), 0.0, 0.0, 0.0);
        for process in ProcessId::ALL { human.biology_mut().unwrap().set_running(process, false); }
        assert!(human.hire(Job::new(JobKind::FoodFactoryWorker, COOKER, OFFICE_HOURS)));
        (world, human)
    }

    #[test]
    fn a_worker_fetches_wheat_and_stores_the_meals_it_cooks() {
        let (mut world, mut human) = setup(3);
        for _ in 0..6000 { world.step(&mut human); }
        assert!(world.bins.stored(PALLET) > 0, "{:?}", world.lines());
        let pack = human.inventory();
        assert_eq!(world.bins.total() + u32::from(pack.count(ItemKind::Wheat)) + u32::from(pack.count(ItemKind::Meal)), 3);
    }

    #[test]
    fn no_wheat_sends_the_worker_back_to_wandering_with_cooking_held_off() {
        let (mut world, mut human) = setup(0);
        world.step(&mut human);
        assert!(human.brain().goals().cooldown(GoalId::Cook) > 0);
        for _ in 0..10 { world.step(&mut human); }
        assert_ne!(human.brain().top_goal(), GoalId::Cook);
        assert_eq!(world.bins.stored(PALLET), 0);
    }

    #[test]
    fn an_unreachable_hopper_does_not_request_a_route_every_tick() {
        let (mut world, mut human) = setup(3);
        for y in 0..12 { world.map.set_terrain(Point::new(4, y), WALL); }
        crate::sim::walker::ROUTES_ASKED.with(|count| count.set(0));
        for _ in 0..2000 { world.step(&mut human); }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|count| count.get());
        assert!(routes < 100, "{routes} routes in 2000 ticks");
        assert_eq!(world.bins.stored(HOPPER), 3);
        assert_eq!(world.bins.stored(PALLET), 0);
    }

    #[test]
    fn hunger_interrupts_cooking_without_spending_wheat_and_work_resumes_afterward() {
        let (mut world, mut human) = setup(1);
        for _ in 0..2500 {
            world.step(&mut human);
            if human.interacting_with() == Some(COOKER) { break; }
        }
        assert_eq!(human.interacting_with(), Some(COOKER));
        human.set_hunger(95.0);
        human.biology_mut().unwrap().set_running(ProcessId::Hunger, true);
        world.step(&mut human);
        assert_ne!(human.brain().top_goal(), GoalId::Cook);
        assert_eq!(human.inventory().count(ItemKind::Wheat), 1);
        assert_eq!(human.inventory().count(ItemKind::Meal), 0);
        human.set_hunger(0.0);
        human.biology_mut().unwrap().set_running(ProcessId::Hunger, false);
        for _ in 0..2500 { world.step(&mut human); }
        assert_eq!(world.bins.stored(PALLET), 1);
    }
}
