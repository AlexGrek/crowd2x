//! [`DrillGoal`]: a soldier's, or its commander's, shift — the training
//! ground the king picked, out in the wasteland (`sim::training`).
//!
//! It plans **from what is true now** and keeps nothing but the walk it is on:
//!
//! There is **no barracks**: nothing is built for a soldier, and it has no
//! place to muster. It lives where it lives, gets ready there and goes
//! straight out.
//!
//! 1. **No drill for it now** — the shift over, or no ground yet — and out in
//!    the wasteland: back inside before dark, to its cell on the roll
//!    ([`FeatureKind::Ranks`]: bookkeeping, a cell of the street) — `WorkRoutine`
//!    keeps it wanted until it is in. Inside, nothing.
//! 2. In the district: **too late to go out** — the walk there leaves less than
//!    [`SHORTEST_DRILL`] of the shift — nothing more today. Otherwise, not
//!    ready: it waits where it stands while `ReadyRoutine` sees to its needs
//!    and puts a meal and a drink in its pocket (at whatever fridge and toilet
//!    are nearest — a trip out starts from anywhere, for a drilled job).
//! 3. **The commander** walks to the middle of the ground and stands there an
//!    hour at a time ([`Task::drill`] with a standing).
//! 4. **A soldier** walks the lap: the waypoint after the one it is nearest
//!    ([`next_index`]), or its own place on the lap when it has just come out,
//!    a waypoint at a time, skipping one somebody stands on. It never counts
//!    laps: where it is on the lap is where it stands.
//!
//! Soldiers do not fight. A deathdog seen is run from, as by anybody outside
//! (`FleeRoutine`, over everything).

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, HOUR};
use crate::sim::feature::FeatureKind;
use crate::sim::job::JobKind;
use crate::sim::training::{next_index, ring, LAP, SOLDIERS};

use super::build::{on_duty, HOMEWARD_CELLS_AN_HOUR};
use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Seek, Stand, WAIT_FOR_A_GAP};

/// The least drill worth going out for, in world hours: with less of the
/// shift left than this on arrival, the day's drilling is done.
const SHORTEST_DRILL: f32 = 0.5;

/// How long the commander stands at a stretch, in world seconds.
pub const STAND_SECONDS: f32 = HOUR;

/// How many waypoints on from the next a soldier looks for a free one.
const LOOK_AHEAD: usize = 3;

/// Drill on the training ground. See the [module docs](self).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DrillGoal {
    /// Where it is walking: its cell on the roll, a waypoint, the middle of the ground.
    seek: Seek,
}

impl DrillGoal {
    pub fn new() -> DrillGoal {
        DrillGoal::default()
    }

    /// Walk back beside `place`. `false` where nothing beside it can be
    /// stood on.
    fn beside(&mut self, ctx: &mut GoalCtx<'_>, place: Point) -> bool {
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
        match stand_beside(ctx, place) {
            Stand::Nowhere => false,
            Stand::Here => true,
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
                true
            }
        }
    }

    /// Plan the next step from what is true now: see the [module docs](self).
    fn plan(&mut self, ctx: &mut GoalCtx<'_>, job: &crate::sim::job::Job) -> GoalProgress {
        let here = ctx.body.center_position();
        let roll = job.workplace;
        let in_the_district = ctx.think.map.contains(here);
        let center = ctx.think.training.center().filter(|_| on_duty(job, ctx.body.uid(), ctx.think));
        let Some(center) = center else {
            // Nothing for it now. Out there, back in before dark.
            if in_the_district || !self.beside(ctx, roll) {
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            return GoalProgress::Working;
        };
        if in_the_district {
            let walk = here.manhattan_distance(center) as f32 / HOMEWARD_CELLS_AN_HOUR;
            let left = job.shift.end() as f32 - ctx.think.clock.hours();
            if left - walk < SHORTEST_DRILL {
                // Too late to go out: it would be there only to turn for home.
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            if !ctx.ready_to_go_out {
                // Got ready where it stands: `ReadyRoutine` takes it to the
                // fridge and the toilet, and it is on its way when done.
                let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                return GoalProgress::Working;
            }
        }
        if job.kind == JobKind::Commander {
            self.seek.choose(center);
            let _ = ctx.tasks.push_back(Task::drill(center, watched(STAND_SECONDS)));
            return GoalProgress::Working;
        }
        let uid = ctx.body.uid();
        let slot = ctx.think.features.cells_of(FeatureKind::Ranks).binary_search(&roll).unwrap_or(0);
        let lap = ring(center);
        let first = next_index(center, here, slot, SOLDIERS);
        let waypoint = (0..LOOK_AHEAD)
            .map(|ahead| lap[(first + ahead) % LAP])
            .find(|&cell| ctx.think.occupancy.occupant(cell).is_none_or(|other| other == uid))
            .unwrap_or(lap[first]);
        self.seek.choose(waypoint);
        let _ = ctx.tasks.push_back(Task::drill(waypoint, 0.0));
        GoalProgress::Working
    }
}

impl GoalExecutor for DrillGoal {
    fn goal(&self) -> GoalId {
        GoalId::Drill
    }

    fn save(&self) -> pb::Executor {
        let DrillGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: the half-walked route is gone, and where it was going
    /// is worked out again from where it stands.
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.seek.forget();
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(&job)) = (ctx.biology, ctx.job) else {
            // Nothing to tire, or nothing to do.
            return GoalProgress::Blocked;
        };
        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::Drill(_) | Task::MoveTo(_))) => {
                ctx.tasks.clear();
                self.seek.forget();
                // A body in the way: a moment, and the next waypoint is worked
                // out again. No way there: given up for now, and the hold
                // spaces the next try.
                if ctx.blocked_by.is_some() {
                    let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                    return GoalProgress::Working;
                }
                return GoalProgress::Blocked;
            }
            // Pulled away: worked out again below, from where it stands.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Drill(drilled))) => {
                let uid = ctx.body.uid();
                if job.kind == JobKind::Commander {
                    ctx.think.log.unit(uid, format!("{uid} stood an hour at the drill at {}, {}", drilled.cell.x, drilled.cell.y));
                    self.seek.forget();
                    return GoalProgress::Achieved;
                }
                if ctx.think.training.center().is_some_and(|center| ring(center)[0] == drilled.cell) {
                    ctx.think.log.unit(uid, format!("{uid} drilled a lap round the commander"));
                }
                self.seek.steady();
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        self.plan(ctx, &job)
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
    use crate::map::{Bounds, Map, Size, FLOOR};
    use crate::sim::biology::ProcessId;
    use crate::sim::clock::{Clock, DAY, TIME_SCALE};
    use crate::sim::item::ItemKind;
    use crate::sim::job::{Job, SOLDIER_HOURS};
    use crate::sim::outposts::Outposts;
    use crate::sim::testing::{needy_human, World};
    use crate::sim::training::{choose_ground, footprint};
    use crate::sim::{GameEntity, Human};

    /// `hours` o'clock on day `day` — day 1 is a Monday.
    fn at(day: u32, hours: f32) -> Clock {
        let world = (day - 1) as f64 * DAY as f64 + (hours as f64 - 8.0) * HOUR as f64;
        Clock::after_watching(world / TIME_SCALE as f64)
    }

    /// Ticks in `hours` of world at the test world's rate.
    fn ticks(world: &World, hours: f32) -> u32 {
        (hours * HOUR / TIME_SCALE / world.dt) as u32
    }

    const ROLL: Point = Point::new(8, 2);

    /// A 16x12 floor in the wasteland of seed 3 with two cells on the roll, at `clock`, and — when `ground` — the king's training ground
    /// picked out in the band above it, the ground between loaded, as a
    /// `GameState` would have it.
    fn parade(ground: bool, clock: Clock) -> (World, Point) {
        let mut map = Map::new(Size::new(16, 12), FLOOR).with_wasteland(3);
        let center = choose_ground(&map, &Outposts::new(), ROLL).unwrap();
        let loaded = Bounds::around([Point::new(0, 0), center]).unwrap().grown(40);
        map.load_chunks(map.navigation_area().union(loaded));
        let mut world = World::new(map);
        world.facilities.features.add(FeatureKind::Ranks, ROLL);
        world.facilities.features.add(FeatureKind::Command, Point::new(9, 2));
        world.clock = clock;
        if ground {
            assert!(world.facilities.training.found(&world.map, center, &Outposts::new(), 1));
        }
        (world, center)
    }

    /// A soldier (or its commander) at `cell` with nothing on its mind but the
    /// drill: every need satisfied, a meal and a drink in its pocket.
    fn drilled_at(kind: JobKind, cell: Point) -> Human {
        let workplace = if kind == JobKind::Commander { Point::new(9, 2) } else { ROLL };
        let mut human = needy_human(cell, 0.0, 0.0, 0.0);
        human.set_stamina(100.0);
        let biology = human.biology_mut().unwrap();
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
            biology.set_running(process, false);
        }
        assert!(human.hire(Job::new(kind, workplace, SOLDIER_HOURS)));
        assert!(human.inventory_mut().stow(ItemKind::Food));
        assert!(human.inventory_mut().stow(ItemKind::Water));
        human
    }

    #[test]
    fn a_soldier_walks_in_circles_round_the_middle_of_the_ground() {
        let (mut world, center) = parade(true, at(2, 7.0));
        let mut soldier = drilled_at(JobKind::Soldier, ROLL);
        let lap = ring(center);
        let mut seen = [false; LAP];
        let mut closest = i32::MAX;
        for _ in 0..ticks(&world, 7.0) {
            world.step_in_time(&mut soldier);
            let here = soldier.body().center_position();
            if let Some(index) = lap.iter().position(|&cell| cell == here) {
                seen[index] = true;
            }
            if footprint(center).grown(8).contains(here) {
                closest = closest.min(here.chebyshev_distance(center));
            }
        }
        assert!(seen.iter().all(|&seen| seen), "a whole lap, every waypoint of it: {seen:?}; {:?}", soldier.brain_fields());
        assert!(closest >= 3, "never on the commander's cell or in the middle: {closest}");
        assert!(world.log_contains("drilled a lap round the commander"), "{:?}", world.lines());
        assert!(world.lines().iter().filter(|line| line.contains("drilled a lap")).count() >= 3, "again and again");
    }

    #[test]
    fn the_commander_stands_in_the_middle_an_hour_at_a_time() {
        let (mut world, center) = parade(true, at(2, 7.0));
        let mut commander = drilled_at(JobKind::Commander, Point::new(9, 2));
        let (mut middle, mut stood) = (0, 0);
        for _ in 0..ticks(&world, 7.0) {
            world.step_in_time(&mut commander);
            middle += (commander.body().center_position() == center) as u32;
            stood = world.lines().iter().filter(|line| line.contains("stood an hour at the drill")).count();
        }
        assert!(middle > ticks(&world, 3.0), "in the middle most of the time: {middle}; {:?}", commander.brain_fields());
        assert!(stood >= 2, "an hour at a time: {stood}");
    }

    #[test]
    fn with_no_ground_picked_a_soldier_is_not_on_duty_and_stays_in() {
        let (mut world, center) = parade(false, at(2, 7.0));
        let mut soldier = drilled_at(JobKind::Soldier, ROLL);
        for _ in 0..ticks(&world, 6.0) {
            world.step_in_time(&mut soldier);
        }
        assert!(!world.log_contains("drilled a lap"), "drilling with nowhere to drill");
        assert!(!footprint(center).grown(8).contains(soldier.body().center_position()));
    }

    #[test]
    fn a_soldier_out_on_the_ground_when_the_shift_is_over_comes_back_in() {
        let (mut world, center) = parade(true, at(2, 14.5));
        let mut soldier = drilled_at(JobKind::Soldier, center.offset(4, 0));
        let mut came_in = false;
        for _ in 0..ticks(&world, 4.0) {
            world.step_in_time(&mut soldier);
            came_in |= world.map.contains(soldier.body().center_position());
        }
        assert!(came_in, "never came back in; at {:?}", soldier.body().center_position());
    }

    #[test]
    fn nobody_drills_on_a_sunday() {
        let (mut world, center) = parade(true, at(7, 6.0));
        let mut soldier = drilled_at(JobKind::Soldier, ROLL);
        for _ in 0..ticks(&world, 8.0) {
            world.step_in_time(&mut soldier);
        }
        assert!(!world.log_contains("drilled a lap"), "drilling on a Sunday");
        assert!(!footprint(center).grown(8).contains(soldier.body().center_position()));
    }
}
