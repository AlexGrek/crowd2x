//! Sow permanent flower beds in the city's public green zones.
use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{Seek, Stand, stand_beside};
use crate::map::Point;
use crate::save::{Invalid, pb};
use crate::sim::brain::tasks::plant_flowers::PLANT_SECONDS;
use crate::sim::feature::FeatureKind;
use crate::sim::flowers::green_plot;

fn available(ctx: &GoalCtx<'_>, cell: Point) -> bool {
    green_plot(ctx.think.map, cell)
        && ctx.think.flowers.at(cell).is_none()
        && ctx.think.properties.of(cell).is_none()
}

fn nearest_plot(ctx: &GoalCtx<'_>) -> Option<Point> {
    let here = ctx.body.center_position();
    let navigation = ctx.think.map.navigation();
    ctx.think
        .features
        .green_plots()
        .iter()
        .copied()
        .filter(|&cell| {
            available(ctx, cell)
                && !ctx
                    .memory
                    .out_of_reach()
                    .contains(cell, ctx.think.clock.elapsed())
                && !ctx.think.occupancy.is_occupied(cell)
                && ctx.think.corpses.at(cell).is_none()
                && cell.cardinal_neighbours().into_iter().any(|side| {
                    ctx.think.is_passable_with(side, ctx.body.home())
                        && navigation.may_reach(here, side)
                })
        })
        .min_by_key(|&cell| (here.manhattan_distance(cell), cell))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DecorateGoal {
    seek: Seek,
}

impl DecorateGoal {
    pub fn new() -> Self {
        Self::default()
    }

    fn plan(&self, ctx: &mut GoalCtx<'_>) -> bool {
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
        let _ = ctx
            .tasks
            .push_back(Task::plant_flowers(target, PLANT_SECONDS));
        true
    }
}

impl GoalExecutor for DecorateGoal {
    fn goal(&self) -> GoalId {
        GoalId::Decorate
    }
    fn save(&self) -> pb::Executor {
        pb::Executor::Seek(self.seek.save())
    }
    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.seek.target.is_some_and(|cell| available(ctx, cell)) {
            self.seek.steady();
        } else {
            self.seek.forget();
        }
    }
    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none()
            || !ctx.job.is_some_and(|job| {
                ctx.think
                    .features
                    .has(FeatureKind::FlowerSupplies, job.workplace)
            })
        {
            return GoalProgress::Blocked;
        }
        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                if self.seek.setback(ctx, nearest_plot) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                ctx.tasks.clear();
                self.seek.forget();
            }
            (TaskResult::Success, Some(Task::PlantFlowers(_))) => {
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        let Some(target) = nearest_plot(ctx) else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(target);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.seek.forget();
            GoalProgress::Blocked
        }
    }
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "flower bed",
            self.seek
                .target
                .map_or_else(|| "none".into(), |cell| format!("{}, {}", cell.x, cell.y)),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Map, Size, TERRAIN, TerrainId, WALL};
    use crate::sim::biology::ProcessId;
    use crate::sim::clock::{Clock, DAY, HOUR, TIME_SCALE};
    use crate::sim::job::{FARM_HOURS, Job};
    use crate::sim::testing::{World, needy_human, prop_at};
    use crate::sim::{GameEntity, Human, JobKind};

    const BASE: Point = Point::new(1, 1);
    fn grass() -> TerrainId {
        TerrainId(
            TERRAIN
                .iter()
                .position(|tile| tile.name == "grass")
                .unwrap() as u16,
        )
    }
    fn park() -> Map {
        let mut map = Map::new(Size::new(14, 10), grass());
        prop_at(&mut map, "flower supplies", BASE);
        map
    }
    fn decorator() -> Human {
        let mut human = needy_human(Point::new(1, 2), 0.0, 0.0, 0.0);
        for process in ProcessId::ALL {
            human.biology_mut().unwrap().set_running(process, false);
        }
        assert!(human.hire(Job::new(JobKind::CityDecorator, BASE, FARM_HOURS)));
        human
    }
    fn at(day: u32) -> Clock {
        Clock::after_watching(((day - 1) as f64 * DAY as f64 + HOUR as f64) / TIME_SCALE as f64)
    }

    #[test]
    fn a_decorator_seeds_public_green_zones_with_different_flowers() {
        let mut map = park();
        // Keep a paved strip clear, even though it is surrounded by grass.
        for x in 0..14 {
            map.set_terrain(Point::new(x, 4), FLOOR);
        }
        map.set_ceiling(Point::new(2, 2), true);
        let mut world = World::new(map);
        world.clock = at(1);
        let mut human = decorator();
        for _ in 0..8000 {
            world.step(&mut human);
        }
        let beds: Vec<_> = world.flowers.iter().collect();
        assert!(beds.len() >= 12, "{} beds: {:?}", beds.len(), world.lines());
        assert!(
            beds.iter()
                .all(|bed| green_plot(&world.map, bed.cell) && bed.cell.y != 4)
        );
        assert!(beds.iter().all(|bed| world.map.is_passable(bed.cell)));
        assert!(
            world.flowers.at(Point::new(2, 2)).is_none(),
            "no planting indoors"
        );
        let varieties: std::collections::BTreeSet<_> = beds.iter().map(|bed| bed.variety).collect();
        assert!(varieties.len() >= 3, "{varieties:?}");
        assert!(world.log_contains("planted"));
    }

    #[test]
    fn private_green_gardens_are_left_out_of_the_planting_index() {
        let mut map = park();
        for y in 1..=6 {
            for x in 6..=11 {
                if x == 6 || x == 11 || y == 1 || y == 6 {
                    map.set_terrain(Point::new(x, y), WALL);
                }
            }
        }
        let door = Point::new(6, 3);
        map.set_terrain(door, grass());
        prop_at(&mut map, crate::sim::door::HOUSE_DOOR, door);
        let world = World::new(map);
        let private = Point::new(8, 4);
        assert!(green_plot(&world.map, private));
        assert!(world.properties.of(private).is_some());
        assert!(!world.features.green_plots().contains(&private));
        assert!(world.features.green_plots().contains(&Point::new(4, 4)));
    }

    #[test]
    fn without_supplies_or_green_zones_a_decorator_backs_off() {
        for map in [
            Map::new(Size::new(14, 10), FLOOR),
            Map::new(Size::new(14, 10), grass()),
        ] {
            let mut world = World::new(map);
            world.clock = at(1);
            let mut human = decorator();
            let before = crate::sim::walker::ROUTES_ASKED.with(|n| n.get());
            let mut held = 0;
            for _ in 0..1500 {
                world.step(&mut human);
                held += u32::from(human.brain().goals().cooldown(GoalId::Decorate) > 0);
            }
            assert!(world.flowers.iter().next().is_none());
            assert!(held > 1000);
            let asked = crate::sim::walker::ROUTES_ASKED.with(|n| n.get()) - before;
            assert!(asked < 100, "{asked} routes in 1500 ticks");
        }
        let mut paved = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut paved, "flower supplies", BASE);
        let mut world = World::new(paved);
        world.clock = at(1);
        let mut human = decorator();
        for _ in 0..500 {
            world.step(&mut human);
        }
        assert!(world.flowers.iter().next().is_none());
        assert!(human.brain().goals().cooldown(GoalId::Decorate) > 0);
    }

    #[test]
    fn unreachable_green_zones_do_not_start_routes_every_tick() {
        let mut map = park();
        // All grass behind a continuous wall; the worker and supplies on paving.
        for y in 0..10 {
            for x in 0..5 {
                map.set_terrain(Point::new(x, y), FLOOR);
            }
            map.set_terrain(Point::new(5, y), WALL);
        }
        let mut world = World::new(map);
        world.clock = at(1);
        let mut human = decorator();
        let before = crate::sim::walker::ROUTES_ASKED.with(|n| n.get());
        for _ in 0..2000 {
            world.step(&mut human);
        }
        assert!(world.flowers.iter().next().is_none());
        let asked = crate::sim::walker::ROUTES_ASKED.with(|n| n.get()) - before;
        assert!(asked < 100, "{asked} routes in 2000 ticks");
    }

    #[test]
    fn decorators_keep_sunday_off_and_stop_planting_when_dismissed() {
        let mut world = World::new(park());
        let mut human = decorator();
        for day in 1..=6 {
            assert!(human.job().unwrap().is_due(at(day)));
        }
        world.clock = at(7);
        for _ in 0..1000 {
            world.step(&mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Decorate);
        }
        assert!(world.flowers.iter().next().is_none());
        world.clock = at(8);
        for _ in 0..700 {
            world.step(&mut human);
        }
        assert!(world.flowers.iter().next().is_some());
        human.dismiss();
        let planted = world.flowers.iter().count();
        for _ in 0..1000 {
            world.step(&mut human);
        }
        assert_eq!(world.flowers.iter().count(), planted);
        assert_ne!(human.brain().top_goal(), GoalId::Decorate);
    }
}
