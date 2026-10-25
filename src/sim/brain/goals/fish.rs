//! Fishing trips: fill a pack at an outside lakeshore, then store the catch.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use crate::sim::brain::tasks::fish::FISH_SECONDS;
use super::{stand_beside, Seek, Stand};
use crate::sim::brain::tasks::DEPOSIT_SECONDS;

fn is_water(ctx: &GoalCtx<'_>, cell: Point) -> bool {
    !ctx.think.map.contains(cell) && ctx.think.map.terrain(cell) == Some(crate::map::WATER)
}

fn nearest_water(ctx: &GoalCtx<'_>) -> Option<Point> {
    let here = ctx.body.center_position();
    let from = ctx.job.filter(|_| ctx.think.map.contains(here)).map_or(here, |job| job.workplace);
    let now = ctx.think.clock.elapsed();
    let navigation = ctx.think.map.navigation();
    ctx.think.features.fishing_water().iter().copied().filter(|&water| {
        !ctx.memory.out_of_reach().contains(water, now)
            && water.cardinal_neighbours().into_iter().any(|bank| {
                ctx.think.is_passable_with(bank, ctx.body.home()) && navigation.may_reach(here, bank)
            })
    }).min_by_key(|&water| (from.manhattan_distance(water), water))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FishGoal {
    seek: Seek,
}

impl FishGoal {
    pub fn new() -> FishGoal {
        FishGoal::default()
    }

    fn bin(ctx: &GoalCtx<'_>) -> Option<Point> {
        let bin = ctx.job?.workplace;
        ctx.think.features.has(FeatureKind::FishBin, bin).then_some(bin)
    }

    fn still_worth_it(ctx: &GoalCtx<'_>, target: Point) -> bool {
        match ctx.job {
            Some(job) if job.workplace == target => Self::bin(ctx).is_some(),
            _ => is_water(ctx, target),
        }
    }

    fn plan(&mut self, ctx: &mut GoalCtx<'_>, then: Option<Task>) -> bool {
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
        if let Some(then) = then {
            let _ = ctx.tasks.push_back(then);
        }
        true
    }

    fn work_at(&self, ctx: &GoalCtx<'_>) -> Option<Task> {
        let target = self.seek.target?;
        if Some(target) == ctx.job.map(|job| job.workplace) {
            let carrying = ctx.inventory.is_some_and(|pack| pack.count(ItemKind::Fish) > 0);
            carrying.then_some(Task::deposit(target, DEPOSIT_SECONDS))
        } else {
            Some(Task::fish(target, FISH_SECONDS))
        }
    }

    fn head_for(&mut self, place: Point) {
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
    }

    fn forget(&mut self) {
        self.seek.forget();
    }

    fn is_beside(ctx: &GoalCtx<'_>, bin: Point) -> bool {
        ctx.body.center_position().manhattan_distance(bin) <= 1
    }
}

impl GoalExecutor for FishGoal {
    fn goal(&self) -> GoalId {
        GoalId::Fish
    }

    fn save(&self) -> pb::Executor {
        let FishGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        match self.seek.target {
            Some(target) if Self::still_worth_it(ctx, target) => self.seek.steady(),
            Some(_) => self.forget(),
            None => {}
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(job), Some(inventory)) = (ctx.biology, ctx.job, ctx.inventory) else {
            return GoalProgress::Blocked;
        };
        let bin = job.workplace;
        if Self::bin(ctx).is_none() {
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                let to_the_bin = self.seek.target == Some(bin);
                let retry = if to_the_bin {
                    self.seek.setback(ctx, |_| None)
                } else {
                    self.seek.setback(ctx, nearest_water)
                };
                if retry && self.plan(ctx, self.work_at(ctx)) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Deposit(_))) => {
                self.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        let carrying = inventory.count(ItemKind::Fish) > 0;
        if !carrying && !inventory.room_for(ItemKind::Fish) {
            return GoalProgress::Blocked;
        }
        if inventory.count(ItemKind::Fish) < 5 && inventory.room_for(ItemKind::Fish) {
            if !carrying
                && !ctx.ready_to_go_out
                && ctx.think.map.contains(ctx.body.center_position())
                && !Self::is_beside(ctx, bin)
                && let Some(bin) = Self::bin(ctx)
                && nearest_water(ctx).is_some()
            {
                self.head_for(bin);
                if self.plan(ctx, None) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            let working = self.seek.target.filter(|&water| water != bin && is_water(ctx, water));
            if let Some(water) = working.or_else(|| nearest_water(ctx)) {
                self.head_for(water);
                if self.plan(ctx, Some(Task::fish(water, FISH_SECONDS))) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            if !carrying {
                self.forget();
                ctx.think.log.unit(ctx.body.uid(), format!("{} found no reachable water for fishing", ctx.body.uid()));
                return GoalProgress::Blocked;
            }
        }

        let Some(bin) = Self::bin(ctx) else {
            self.forget();
            return GoalProgress::Blocked;
        };
        self.head_for(bin);
        if self.plan(ctx, Some(Task::deposit(bin, DEPOSIT_SECONDS))) {
            GoalProgress::Working
        } else {
            self.forget();
            GoalProgress::Blocked
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
    use crate::sim::biology::ProcessId;
    use crate::sim::clock::{Clock, DAY, HOUR, TIME_SCALE};
    use crate::sim::job::{Job, JobKind, FARM_HOURS};
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::{GameEntity, Human};

    const BIN: Point = Point::new(4, 3);

    fn at(day: u32) -> Clock {
        Clock::after_watching(((day - 1) as f64 * DAY as f64 + HOUR as f64) / TIME_SCALE as f64)
    }

    fn yard(wasteland: bool, day: u32) -> World {
        let mut map = Map::new(Size::new(9, 7), FLOOR);
        if wasteland { map = map.with_wasteland(0); }
        prop_at(&mut map, "fish bin", BIN);
        map.load_chunks(map.navigation_area());
        let mut world = World::new(map);
        world.clock = at(day);
        world
    }

    fn fisherman() -> Human {
        let mut human = needy_human(Point::new(3, 3), 0.0, 0.0, 0.0);
        human.set_stamina(100.0);
        for process in ProcessId::ALL {
            human.biology_mut().unwrap().set_running(process, false);
        }
        assert!(human.hire(Job::new(JobKind::Fisherman, BIN, FARM_HOURS)));
        human
    }

    #[test]
    fn a_fisherman_leaves_the_district_fishes_on_dry_land_and_stores_five_fish() {
        let mut world = yard(true, 1);
        let mut human = fisherman();
        let mut fished = false;
        for _ in 0..15000 {
            world.step_in_time(&mut human);
            assert!(human.inventory().count(ItemKind::Fish) <= 5);
            if let Some(Task::Fish(cast)) = human.brain().current_task() {
                fished = true;
                assert!(!world.map.contains(cast.water));
                assert_eq!(world.map.terrain(cast.water), Some(crate::map::WATER));
                assert_eq!(human.center_position().manhattan_distance(cast.water), 1);
                assert!(world.map.is_passable(human.center_position()));
            }
            if world.bins.stored(BIN) > 0 { break; }
        }
        assert!(fished, "{:?}", human.brain_fields());
        assert_eq!(world.bins.stored(BIN), 5, "{:?}", world.lines());
        assert_eq!(human.inventory().count(ItemKind::Fish), 0);
        assert_eq!(human.center_position().manhattan_distance(BIN), 1);
    }

    #[test]
    fn fishermen_work_saturday_and_rest_only_on_sunday() {
        for day in 1..=7 {
            let mut world = yard(true, day);
            let mut human = fisherman();
            let mut working = false;
            for _ in 0..50 {
                world.step_in_time(&mut human);
                working |= human.brain().top_goal() == GoalId::Fish;
            }
            assert_eq!(working, day != 7, "day {day}");
        }
    }

    #[test]
    fn missing_or_unreachable_water_is_held_off_without_repeated_routing() {
        for outside in [false, true] {
            let mut world = yard(outside, 1);
            if outside {
                for x in 0..9 { world.map.set_terrain(Point::new(x, 0), WALL); world.map.set_terrain(Point::new(x, 6), WALL); }
                for y in 0..7 { world.map.set_terrain(Point::new(0, y), WALL); world.map.set_terrain(Point::new(8, y), WALL); }
            }
            let mut human = fisherman();
            crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
            for _ in 0..600 { world.step_in_time(&mut human); }
            assert_eq!(world.bins.stored(BIN), 0);
            assert!(world.log_contains("found no reachable water for fishing"));
            assert!(human.brain().goals().cooldown(GoalId::Fish) > 0);
            assert!(crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get()) < 100);
        }
    }

    #[test]
    fn a_full_pack_is_delivered_without_fishing_and_a_dismissed_worker_stops() {
        let mut world = yard(true, 1);
        let mut human = fisherman();
        for _ in 0..5 { assert!(human.inventory_mut().stow(ItemKind::Fish)); }
        assert!(!human.inventory_mut().stow(ItemKind::Fish));
        for _ in 0..300 {
            world.step_in_time(&mut human);
            assert!(!matches!(human.brain().current_task(), Some(Task::Fish(_))));
            if world.bins.stored(BIN) > 0 { break; }
        }
        assert_eq!(world.bins.stored(BIN), 5);
        human.dismiss();
        for _ in 0..100 {
            world.step_in_time(&mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Fish);
        }
    }
}
