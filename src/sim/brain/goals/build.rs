//! [`BuildGoal`]: a builder's shift — the outpost going up, a tile at a time
//! (`sim::outposts`).
//!
//! It plans **from what is true now** — the plan's tiles, which one is its
//! own, what it carries, where it stands — and keeps nothing but the walk it
//! is on:
//!
//! 1. **No work for it now** — the shift over, or no tile it could take on —
//!    and out in the wasteland: back to its yard, inside, before dark
//!    (`WorkRoutine` keeps it wanted until it is in). Inside, nothing.
//! 2. **Resting** after a stretch (`Memory::is_resting`): the rest of the
//!    rest, standing.
//! 3. **No tile of its own**: take one on ([`Task::claim`]) — the plan's
//!    [`pick`](crate::sim::outposts::Outposts::pick) for where it stands and
//!    what it carries. The world says next tick whether it got it.
//! 4. **Too late to go out**, in the district: a trip that would not leave
//!    it a stretch out there before it must turn for home is not started.
//!    Otherwise, not ready: its yard first — **every trip out starts
//!    there**, where getting ready happens (`ReadyRoutine`, a meal and a
//!    drink in the pocket, as a collector's at its bin).
//! 5. **Steel**: the nearest steel rack with any, a piece at a time
//!    ([`Task::fetch`]), while its tile wants steel it has not got — and, in
//!    the district, **a trip's worth** ([`TRIP_STEEL`]): a share of what the
//!    tiles nobody has taken on want, so the next tile out there wants no
//!    walk back. The spare pieces only while the rack holds enough for every
//!    tile still on the plan besides, so no builder's tile ever waits on
//!    steel in another's pocket.
//! 6. Beside its tile, from a cell no tile of the plan is in: **lay** its
//!    building tile ([`Task::lay`]), or **work on it** ([`Task::construct`])
//!    for a stretch — an hour at the most, no more than the tile still wants,
//!    and no later than it must leave to be in before dark ([`knock_off`]) —
//!    and then rest ([`REST`](crate::sim::outposts::REST)). The day's
//!    building done, out there, back to its yard.
//!
//! A tile it cannot lay — somebody standing in it — is waited for
//! ([`Seek::wait_again`]), then given up on for now (held off). A builder on
//! leave gives its tile back (`Effect::GiveUpTile`, from `Brain::step`).

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, HOUR, TIME_SCALE};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;
use crate::sim::job::Job;
use crate::sim::kinds::{HUMAN_SPEED, PACE_RANGE};
use crate::sim::outposts::{Stage, Tile, BUILDERS, REST, STRETCH, TILE_WORK};
use crate::sim::sun::SUNSET;
use crate::sim::uid::Uid;
use crate::sim::Think;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::{LAY_SECONDS, WITHDRAW_SECONDS};
use super::{nearest_store, stand_beside, Seek, Stand};

/// The shortest stretch worth starting, in world seconds: with less of the
/// shift left than this, the day's building is done.
const SHORTEST_STRETCH: f32 = 5.0 * crate::sim::clock::MINUTE;

/// The most steel a builder sets out with: about what a day out there
/// builds — a tile is two hours' work and a rest — when the outpost is near.
/// An outpost three hours' walk out (seed 7's first) is two tiles a day; one
/// a trip, it was one.
pub const TRIP_STEEL: u32 = 3;

/// The hour a builder out in the wasteland is to be back in by: half an hour
/// before sunset, ahead of the dark and the deathdogs that come out with it.
const IN_BY: f32 = SUNSET - 0.5;

/// The cells of the way home a builder counts on walking in a world hour: at
/// the slowest pace anybody has, a tenth off for the bends in the route — so
/// it turns for home in time whoever it is.
const HOMEWARD_CELLS_AN_HOUR: f32 = HUMAN_SPEED * (1.0 - PACE_RANGE) * HOUR / TIME_SCALE / 1.1;

/// The hour a builder `home` cells from its yard stops building for the day:
/// the end of its shift, or early enough to walk in by [`IN_BY`] — whichever
/// is first. An outpost an hour out leaves the whole shift; three hours out,
/// it is in by eight only if it stops before four.
pub fn knock_off(shift_end: u8, home: u32) -> f32 {
    (shift_end as f32).min(IN_BY - home as f32 / HOMEWARD_CELLS_AN_HOUR)
}

/// Whether somebody in `job` has building to do now: the shift is due, and —
/// worked only on demand — there is a tile for it, its own or one nobody has.
/// What `WorkRoutine` and `ReadyRoutine` ask of an on-demand job.
pub fn on_duty(job: &Job, uid: Uid, think: &Think<'_>) -> bool {
    job.is_due(think.clock) && (!job.kind.is_on_demand() || think.outposts.has_work_for(uid))
}

/// Build the outpost going up. See the [module docs](self).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BuildGoal {
    /// Where it is walking: its yard, a steel rack, a cell beside its tile.
    seek: Seek,
}

impl BuildGoal {
    pub fn new() -> BuildGoal {
        BuildGoal::default()
    }

    /// Head for `place`, unless already on the way.
    fn head_for(&mut self, place: Point) {
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
    }

    /// Walk beside `place` (unless there already) and `then`. `false` where
    /// nothing beside it can be stood on.
    fn beside(&mut self, ctx: &mut GoalCtx<'_>, place: Point, then: Option<Task>) -> bool {
        self.head_for(place);
        match stand_beside(ctx, place) {
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

    /// Walk to `cell` itself (unless there already) and `then`.
    fn onto(&mut self, ctx: &mut GoalCtx<'_>, cell: Point, then: Task) {
        self.head_for(cell);
        if ctx.body.center_position() != cell {
            let _ = ctx.tasks.push_back(Task::move_to(cell));
        }
        let _ = ctx.tasks.push_back(then);
    }

    /// The cell to work on the tile in `tile` from: one beside it no tile of
    /// the plan is in, that can be walked — where it stands, if it is one;
    /// else one nobody else stands in before one somebody does, the nearest,
    /// the lower cell on a tie.
    fn stand_for(ctx: &GoalCtx<'_>, tile: Point) -> Option<Point> {
        let here = ctx.body.center_position();
        let uid = ctx.body.uid();
        let stands = ctx.think.outposts.stand_cells(tile).filter(|&cell| ctx.think.map.is_passable(cell));
        stands.min_by_key(|&cell| {
            let taken = ctx.think.occupancy.occupant(cell).is_some_and(|other| other != uid);
            (cell != here, taken, cell.manhattan_distance(here), cell)
        })
    }

    /// World seconds of building left today for a builder working at `cell`:
    /// until it must turn for home ([`knock_off`]). Negative once past it.
    fn day_left(ctx: &GoalCtx<'_>, job: &Job, cell: Point) -> f32 {
        let home = cell.manhattan_distance(job.workplace) as u32;
        (knock_off(job.shift.end(), home) - ctx.think.clock.hours()) * HOUR
    }

    /// World seconds the walk from `from` to `to` is counted on taking.
    fn walk(from: Point, to: Point) -> f32 {
        from.manhattan_distance(to) as f32 / HOMEWARD_CELLS_AN_HOUR * HOUR
    }

    /// The steel to set out for `tile` with, the `rack` in reach: what the
    /// tile wants, and — while the rack holds more than every tile still on
    /// the plan wants, and the pack has room to spare — a share of what the
    /// tiles nobody has taken on want, up to [`TRIP_STEEL`].
    fn trip_steel(ctx: &GoalCtx<'_>, tile: &Tile, rack: Point) -> u32 {
        let own = tile.steel_wanted();
        let (all, free) = ctx.think.outposts.steel_wanted();
        let spare_on_the_rack = ctx.think.bins.stored(rack) > all;
        let room = ctx.inventory.is_some_and(|pack| pack.room_for_many(ItemKind::Steel, 2));
        if !spare_on_the_rack || !room {
            return own;
        }
        (own + free.div_ceil(BUILDERS as u32)).min(TRIP_STEEL).max(own)
    }

    /// The day's building is done: out there, back to its yard, in before
    /// dark; inside, nothing more today.
    fn done_for_the_day(&mut self, ctx: &mut GoalCtx<'_>, yard: Point, in_the_district: bool) -> GoalProgress {
        if !in_the_district && self.beside(ctx, yard, None) {
            return GoalProgress::Working;
        }
        self.seek.forget();
        GoalProgress::Blocked
    }

    /// Plan the next step from what is true now: see the [module docs](self).
    fn plan(&mut self, ctx: &mut GoalCtx<'_>, job: &Job) -> GoalProgress {
        let uid = ctx.body.uid();
        let here = ctx.body.center_position();
        let yard = job.workplace;
        let in_the_district = ctx.think.map.contains(here);
        if !on_duty(job, uid, ctx.think) {
            // Nothing for it now. Out there, back in before dark.
            if in_the_district || !self.beside(ctx, yard, None) {
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            return GoalProgress::Working;
        }
        let now = ctx.think.clock.elapsed();
        if ctx.memory.is_resting(now) {
            let _ = ctx.tasks.push_back(Task::wait(watched(ctx.memory.rest_left(now) as f32)));
            return GoalProgress::Working;
        }
        let carried = ctx.inventory.map_or(0, |pack| pack.count(ItemKind::Steel) as u32);
        let Some(tile) = ctx.think.outposts.claim_of(uid).copied() else {
            let Some(pick) = ctx.think.outposts.pick(here, carried, !in_the_district) else {
                self.seek.forget();
                return GoalProgress::Blocked;
            };
            let _ = ctx.tasks.push_back(Task::claim(pick));
            return GoalProgress::Working;
        };
        if in_the_district && Self::day_left(ctx, job, tile.cell) - Self::walk(here, tile.cell) < SHORTEST_STRETCH {
            // Too late to go out: it would be there only to turn for home.
            return self.done_for_the_day(ctx, yard, in_the_district);
        }
        let at_the_yard = here.manhattan_distance(yard) <= 1;
        if in_the_district && !ctx.ready_to_go_out {
            // Every trip out starts at the yard, and is got ready for there.
            if at_the_yard {
                let _ = ctx.tasks.push_back(Task::wait(super::WAIT_FOR_A_GAP));
                return GoalProgress::Working;
            }
            if self.beside(ctx, yard, None) {
                return GoalProgress::Working;
            }
            self.seek.forget();
            return GoalProgress::Blocked;
        }
        let short = carried < tile.steel_wanted();
        let rack = if short || in_the_district { nearest_store(ctx, FeatureKind::SteelRack, true) } else { None };
        // Short for its tile, or setting out with less than a trip's worth.
        if short || rack.is_some_and(|rack| carried < Self::trip_steel(ctx, &tile, rack)) {
            let Some(rack) = rack else {
                ctx.think.log.unit(uid, format!("{uid} found no steel to build with"));
                self.seek.forget();
                return GoalProgress::Blocked;
            };
            if self.beside(ctx, rack, Some(Task::fetch(rack, WITHDRAW_SECONDS))) {
                return GoalProgress::Working;
            }
            self.seek.forget();
            return GoalProgress::Blocked;
        }
        let Some(stand) = Self::stand_for(ctx, tile.cell) else {
            self.seek.forget();
            return GoalProgress::Blocked;
        };
        let left_today = Self::day_left(ctx, job, stand);
        let work = match tile.stage {
            Stage::Planned if left_today >= SHORTEST_STRETCH => Task::lay(tile.cell, LAY_SECONDS),
            Stage::Planned => return self.done_for_the_day(ctx, yard, in_the_district),
            Stage::Laid { progress } => {
                let stretch = STRETCH.min(TILE_WORK - progress).min(left_today);
                if stretch < SHORTEST_STRETCH.min(TILE_WORK - progress) {
                    return self.done_for_the_day(ctx, yard, in_the_district);
                }
                Task::construct(tile.cell, watched(stretch))
            }
            Stage::Built => return GoalProgress::Working,
        };
        self.onto(ctx, stand, work);
        GoalProgress::Working
    }
}

impl GoalExecutor for BuildGoal {
    fn goal(&self) -> GoalId {
        GoalId::Build
    }

    fn save(&self) -> pb::Executor {
        let BuildGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: the half-walked route is gone, and where it was going
    /// is worked out again from the plan as it is now.
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.seek.forget();
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(&job), Some(_)) = (ctx.biology, ctx.job, ctx.inventory) else {
            // Nothing to tire, nothing to do, or nothing to carry steel in.
            return GoalProgress::Blocked;
        };
        let uid = ctx.body.uid();
        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there:
                // given up for now, and the hold spaces the next try.
                if self.seek.setback(ctx, |_| None) {
                    // The wait is queued; the walk is planned again after it.
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            // Pulled away, a tile taken by somebody else first, the rack
            // emptied: worked out again below, from what is true now.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Lay(lay))) => {
                let still_planned = ctx.think.outposts.claim_of(uid).is_some_and(|tile| tile.cell == lay.tile && tile.stage == Stage::Planned);
                if still_planned {
                    // The world would not have it laid: somebody stood there.
                    if self.seek.wait_again(ctx) {
                        return GoalProgress::Working;
                    }
                    self.seek.forget();
                    return GoalProgress::Blocked;
                }
                self.seek.steady();
            }
            (TaskResult::Success, Some(Task::Construct(stretch))) => {
                if let Some(tile) = ctx.think.outposts.claim_of(uid).filter(|tile| tile.cell == stretch.tile) {
                    ctx.think.log.unit(uid, format!(
                        "{uid} worked on the outpost's {} at {}, {}: {:.0}% built",
                        tile.part.name(),
                        stretch.tile.x,
                        stretch.tile.y,
                        tile.done() * 100.0
                    ));
                }
                let now = ctx.think.clock.elapsed();
                ctx.memory.rest_until(Some(now + REST as f64));
                self.seek.forget();
                return GoalProgress::Achieved;
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
    use crate::map::{Bounds, Map, Size, FLOOR, WALL};
    use crate::sim::biology::ProcessId;
    use crate::sim::clock::{Clock, DAY, TIME_SCALE};
    use crate::sim::job::{JobKind, BUILDER_HOURS};
    use crate::sim::outposts::{first_clear_site, footprint, Outposts, Part};
    use crate::sim::testing::{needy_human, prop_at, World};
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

    const RACK: Point = Point::new(3, 3);

    /// A 16x12 floor in the wasteland of seed 3 with a throne and a steel
    /// rack holding `steel` — and so a builders' yard beside it — at
    /// `clock`, with an outpost decreed out in the band above it and the
    /// ground between loaded, as a `GameState` would have it.
    fn site(steel: u32, clock: Clock) -> (World, Point) {
        let mut map = Map::new(Size::new(16, 12), FLOOR).with_wasteland(3);
        prop_at(&mut map, "throne", Point::new(12, 8));
        prop_at(&mut map, "steel rack", RACK);
        let center = first_clear_site(&map);
        let ground = Bounds::around([Point::new(0, 0), center]).unwrap().grown(40);
        map.load_chunks(map.navigation_area().union(ground));
        let mut world = World::new(map);
        let _ = world.facilities.bins.add(RACK, steel);
        world.clock = clock;
        assert!(world.facilities.outposts.found(&world.map, center, 1));
        (world, center)
    }

    /// A builder at `cell` with nothing on its mind but the work: every need
    /// satisfied and every process but tiring off, hired to the yard.
    fn builder_at(world: &World, cell: Point) -> Human {
        let yard = world.features.cells_of(FeatureKind::BuildersYard)[0];
        let mut human = needy_human(cell, 0.0, 0.0, 0.0);
        human.set_stamina(100.0);
        let biology = human.biology_mut().unwrap();
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
            biology.set_running(process, false);
        }
        assert!(human.hire(Job::new(JobKind::Builder, yard, BUILDER_HOURS)));
        human
    }

    /// [`builder_at`] its yard.
    fn builder(world: &World) -> Human {
        builder_at(world, world.features.cells_of(FeatureKind::BuildersYard)[0])
    }

    /// A builder standing beside the outpost's first wall, the wall its own
    /// and its building tile down, with a meal in its pocket.
    fn at_a_laid_wall(world: &mut World) -> (Human, Point) {
        let wall = *world.outposts.under_way().unwrap().tiles().iter().find(|tile| tile.part == Part::Wall).unwrap();
        let stand = world.outposts.stand_cells(wall.cell).find(|&cell| world.map.is_passable(cell)).unwrap();
        let mut human = builder_at(world, stand);
        assert!(world.facilities.outposts.claim(wall.cell, human.uid()));
        world.facilities.outposts.lay(&mut world.map, wall.cell, human.uid()).unwrap();
        assert!(human.inventory_mut().stow(ItemKind::Food));
        (human, wall.cell)
    }

    fn count(world: &World, needle: &str) -> usize {
        world.lines().iter().filter(|line| line.contains(needle)).count()
    }

    /// The whole of a wall: taken on, a piece of steel from the rack, out to
    /// the outpost, its building tile laid, worked an hour, rested, worked
    /// again until two hours are done — and a wall on the map.
    #[test]
    fn a_builder_takes_a_wall_on_fetches_its_steel_lays_it_and_builds_it_in_two_stretches_with_a_rest_between() {
        let (mut world, center) = site(5, at(1, 7.5));
        let mut human = builder(&world);
        let (mut laid, mut built) = (None, None);
        for _ in 0..ticks(&world, 8.0) {
            world.step_in_time(&mut human);
            if laid.is_none() && count(&world, "laid the building tile") > 0 {
                laid = Some(world.clock.elapsed());
            }
            if count(&world, "built the outpost's wall") > 0 {
                built = Some(world.clock.elapsed());
                break;
            }
        }
        let (Some(laid), Some(built)) = (laid, built) else {
            panic!("never built; {:?}\n{:?}", world.lines(), human.brain_fields())
        };
        assert_eq!(count(&world, "took on the outpost's wall"), 1, "{:?}", world.lines());
        assert_eq!(world.bins.stored(RACK), 4, "one piece of steel taken");
        assert_eq!(human.inventory().count(ItemKind::Steel), 0, "and all of it laid");
        assert_eq!(count(&world, "worked on the outpost's wall"), 1, "a stretch of an hour, then the rest: {:?}", world.lines());
        assert!(world.log_contains("50% built"), "{:?}", world.lines());
        let took = (built - laid) as f32;
        assert!(took >= TILE_WORK + REST - 60.0, "two hours' work and a rest between: {} hours", took / HOUR);
        assert!(took <= TILE_WORK + REST + 0.5 * HOUR, "and no idling: {} hours", took / HOUR);
        let outpost = world.outposts.iter().next().unwrap();
        let wall = outpost.tiles().iter().find(|tile| tile.stage == Stage::Built).unwrap().cell;
        assert!(footprint(center).contains(wall));
        assert_eq!(world.map.terrain(wall), Some(WALL));
        assert!(!world.map.is_passable(wall));
        assert!(world.outposts.claim_of(human.uid()).is_none_or(|tile| tile.cell != wall), "given up once built");
    }

    /// No outpost going up, no building: the builder is as good as
    /// unemployed — it never wants its job's goal on the site, and is never
    /// on duty. (Wandered off it, it is walked back in.)
    #[test]
    fn with_nothing_to_build_a_builder_does_not_go_to_work() {
        let (mut world, _) = site(30, at(1, 9.0));
        world.facilities.outposts = Outposts::new();
        let mut human = builder(&world);
        for _ in 0..ticks(&world, 1.0) {
            world.step_in_time(&mut human);
            if world.map.contains(human.center_position()) {
                assert_ne!(human.brain().top_goal(), GoalId::Build, "{:?} at {:?}", human.brain_fields(), human.center_position());
            }
            assert!(human.brain().timesheet().on_duty().is_none(), "never on duty");
        }
    }

    /// Out at the outpost when the shift ends, a builder walks back in —
    /// and wants nothing more of its job once it is.
    #[test]
    fn out_at_the_outpost_after_six_a_builder_comes_back_in() {
        let (mut world, center) = site(5, at(1, 18.2));
        let out = center.offset(0, crate::sim::outposts::HEIGHT);
        let mut human = builder_at(&world, out);
        let mut inside = false;
        for _ in 0..ticks(&world, 3.0) {
            world.step_in_time(&mut human);
            if world.map.contains(human.center_position()) {
                inside = true;
                break;
            }
        }
        assert!(inside, "still out at {:?}: {:?}", human.center_position(), human.brain_fields());
        for _ in 0..10 {
            world.step_in_time(&mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Build, "in: nothing more to do");
    }

    /// Resting after a stretch, a builder wants the work less than a need it
    /// has committed to: hungry, it eats what it carries where it stands.
    #[test]
    fn resting_a_builder_eats_what_it_carries() {
        let (mut world, _) = site(5, at(1, 11.0));
        let (mut human, _) = at_a_laid_wall(&mut world);
        human.biology_mut().unwrap().set_running(ProcessId::Hunger, true);
        human.set_hunger(65.0);
        let now = world.clock.elapsed();
        human.brain_mut().memory_mut().rest_until(Some(now + REST as f64));
        for _ in 0..ticks(&world, 0.5) {
            world.step_in_time(&mut human);
            if world.log_contains("ate what it was carrying") {
                break;
            }
        }
        assert!(world.log_contains("ate what it was carrying"), "{:?}\n{:?}", world.lines(), human.brain_fields());
    }

    /// Hungry the same, but working — not resting — the builder works on:
    /// a stretch under way holds against a need committed to.
    #[test]
    fn working_a_builder_does_not_stop_for_a_need_it_can_wait_on() {
        let (mut world, _) = site(5, at(1, 11.0));
        let (mut human, _) = at_a_laid_wall(&mut world);
        for _ in 0..ticks(&world, 0.1) {
            world.step_in_time(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Build, "{:?}", human.brain_fields());
        human.biology_mut().unwrap().set_running(ProcessId::Hunger, true);
        human.set_hunger(65.0);
        for _ in 0..ticks(&world, 0.3) {
            world.step_in_time(&mut human);
        }
        assert!(!world.log_contains("ate what it was carrying"), "{:?}", world.lines());
        assert_eq!(human.brain().top_goal(), GoalId::Build);
    }

    /// The world lays a building tile only for the builder whose tile it is,
    /// with the steel it takes in the pack — all of it out at once — and not
    /// with somebody standing in the cell.
    #[test]
    fn the_world_lays_a_building_tile_only_with_the_steel_and_the_cell_free() {
        use crate::sim::effect::{apply, Effect};
        let (mut world, _) = site(5, at(1, 11.0));
        let wall = *world.outposts.under_way().unwrap().tiles().iter().find(|tile| tile.part == Part::Wall).unwrap();
        let stand = world.outposts.stand_cells(wall.cell).find(|&cell| world.map.is_passable(cell)).unwrap();
        let mut human = builder_at(&world, stand);
        assert!(world.facilities.outposts.claim(wall.cell, human.uid()));
        let lay = |world: &mut World, human: &mut Human| {
            let World { map, facilities, occupancy, log, .. } = world;
            apply(Effect::Lay { at: wall.cell }, human, map, facilities, occupancy, log);
        };
        let stage = |world: &World| world.outposts.tile(wall.cell).unwrap().stage;
        lay(&mut world, &mut human);
        assert_eq!(stage(&world), Stage::Planned, "no steel, no building tile");
        assert!(human.inventory_mut().stow(ItemKind::Steel) && human.inventory_mut().stow(ItemKind::Steel));
        let somebody = crate::sim::uid::Uid::new(crate::sim::uid::EntityType::Human, 77);
        assert!(world.occupancy.claim(wall.cell, somebody).is_ok());
        lay(&mut world, &mut human);
        assert_eq!(stage(&world), Stage::Planned, "nor with somebody standing there");
        world.occupancy.release(wall.cell, somebody);
        lay(&mut world, &mut human);
        assert_eq!(stage(&world), Stage::Laid { progress: 0.0 });
        assert_eq!(human.inventory().count(ItemKind::Steel), 1, "the one piece a wall takes");
        assert!(!world.map.is_passable(wall.cell));
    }

    /// With more steel on the rack than the whole outpost wants, a builder
    /// sets out with a trip's worth — its own wall's piece and spares for the
    /// next tiles, so they want no walk back — and lays only its wall's.
    #[test]
    fn with_steel_to_spare_a_builder_sets_out_with_a_trips_worth() {
        let (mut world, _) = site(40, at(1, 7.5));
        let mut human = builder(&world);
        for _ in 0..ticks(&world, 6.0) {
            world.step_in_time(&mut human);
            if count(&world, "laid the building tile") > 0 {
                break;
            }
        }
        assert!(world.log_contains("laid the building tile of the outpost's wall"), "{:?}\n{:?}", world.lines(), human.brain_fields());
        assert_eq!(world.bins.stored(RACK), 40 - TRIP_STEEL, "a trip's worth taken: {:?}", world.lines());
        assert_eq!(human.inventory().count(ItemKind::Steel) as u32, TRIP_STEEL - 1,"the wall's piece laid, the spares kept");
    }

    /// With no more on the rack than the outpost wants, nothing spare is
    /// taken: another builder's tile is never left waiting on steel in this
    /// one's pocket.
    #[test]
    fn with_no_steel_to_spare_a_builder_takes_only_its_tiles() {
        let (mut world, _) = site(crate::sim::outposts::OUTPOST_STEEL, at(1, 7.5));
        let mut human = builder(&world);
        for _ in 0..ticks(&world, 6.0) {
            world.step_in_time(&mut human);
            if count(&world, "laid the building tile") > 0 {
                break;
            }
        }
        assert!(world.log_contains("laid the building tile of the outpost's wall"), "{:?}", world.lines());
        assert_eq!(world.bins.stored(RACK), crate::sim::outposts::OUTPOST_STEEL - 1, "one piece, for its wall");
        assert_eq!(human.inventory().count(ItemKind::Steel), 0);
    }

    /// A builder on leave gives back the tile it had taken on, as it stands,
    /// for another builder to take on — off for the day, or for days, it is
    /// not keeping the others off it.
    #[test]
    fn a_builder_on_leave_gives_its_tile_back() {
        let (mut world, _) = site(5, at(1, 9.0));
        let (mut human, wall) = at_a_laid_wall(&mut world);
        human.brain_mut().memory_mut().medical.sick_day = Some(world.clock.day());
        for _ in 0..3 {
            world.step_in_time(&mut human);
        }
        assert!(world.outposts.claim_of(human.uid()).is_none(), "still its: {:?}", world.lines());
        assert!(world.log_contains(&format!("gave up the outpost's wall at {}, {}: on leave", wall.x, wall.y)), "{:?}", world.lines());
        let tile = world.outposts.tile(wall).unwrap();
        assert!(matches!(tile.stage, Stage::Laid { .. }), "as it stands: {:?}", tile.stage);
        assert!(world.outposts.has_work_for(Uid::new(crate::sim::uid::EntityType::Human, 99)), "free for another");
    }

    /// The hour a builder stops: the end of its shift with the outpost an
    /// hour out, early enough to be in by half past seven with it three hours
    /// out — at the slowest pace there is.
    #[test]
    fn a_builder_far_out_stops_early_enough_to_be_in_before_dark() {
        assert_eq!(knock_off(BUILDER_HOURS.end(), 100), BUILDER_HOURS.end() as f32, "an hour out: the whole shift");
        let far = knock_off(BUILDER_HOURS.end(), 340);
        assert!(far < 16.5, "seed 7's outpost, three hours out: stops at {far}");
        let slowest = HUMAN_SPEED * (1.0 - PACE_RANGE) * HOUR / TIME_SCALE;
        assert!(far + 340.0 / slowest <= IN_BY + 0.01, "in by {IN_BY} at the slowest pace: {far}");
    }

    /// No steel anywhere for a wall: said once and held off, not looked for
    /// every tick.
    #[test]
    fn with_no_steel_to_build_with_a_builder_is_held_off() {
        let (mut world, _) = site(0, at(1, 9.0));
        let mut human = builder(&world);
        for _ in 0..ticks(&world, 0.5) {
            world.step_in_time(&mut human);
        }
        let said = count(&world, "found no steel to build with");
        assert!(said >= 1, "{:?}", world.lines());
        assert!(said < 10, "held off between tries: {said}");
    }
}
