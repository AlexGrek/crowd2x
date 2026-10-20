//! [`CollectGoal`]: a scrap collector's shift — out to the heaps in the
//! wasteland, a packful of scrap metal, and back to store it in its own bin.
//!
//! One trip is the goal's unit of work, as an hour at the desk is
//! `WorkGoal`'s: it reports `Achieved` when a packful is in the bin, and if
//! the routine still wants it (`WorkRoutine`, while the shift is due) the next
//! trip starts from wherever the collector stands. **How many trips happen is
//! the routine's to say**; this goal never reads the clock. A collector let go
//! by the end of the shift with scrap still in its pack carries it home and
//! delivers it on the next shift.
//!
//! **It plans from what is true now** (rule 4) and never counts:
//!
//! * **room in the pack** — the heap it was working, while there is anything
//!   left in it; otherwise the nearest heap it can get to ([`nearest_heap`]):
//!   [`MoveTo`](crate::sim::brain::tasks::MoveTo) beside it and
//!   [`Salvage`](crate::sim::brain::tasks::Salvage) a piece, for work
//!   ([`Task::salvage_for_work`]), one piece a task — a piece lands in the
//!   pack at the end of the tick its work finished in (the world step's
//!   doing), so the tick after, the heap and the pack say whether to work
//!   another;
//! * **a full pack, or no heap left and something carried** — its own bin
//!   (the job's workplace): beside it, and
//!   [`Deposit`](crate::sim::brain::tasks::Deposit) everything;
//! * **no heap and nothing carried** — nothing to do: `Blocked`, and the
//!   goal's back-off spaces out the next look. **Heaps do not grow back**
//!   (`map::wasteland`), so a district's yards empty over the days and its
//!   collectors are held off from then on.
//!
//! A heap it cannot get to is given up on for the next nearest
//! ([`Seek::setback`]); its own bin has no other, so a bin it cannot get to
//! gives the trip up. A heap emptied by somebody else under it is a cheap
//! replan.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::salvage::SALVAGE_SECONDS;
use super::{stand_beside, Seek, Stand};
use crate::sim::brain::tasks::DEPOSIT_SECONDS;

/// The heap of scrap nearest this unit's bin while it is in the district (or
/// nearest the unit, out in the wasteland), as the crow flies, that it could
/// get to: one with a side it may stand on that the map's components say it can
/// reach ([`Navigation::may_reach`](crate::map::Navigation::may_reach), an
/// O(1) lookup) and that it does not remember as out of reach. Ties go to
/// the lower cell.
///
/// **Among the heaps of the navigation area** (the site and
/// [`NAV_MARGIN`](crate::map::NAV_MARGIN) round it), which a `GameState`
/// always has loaded: a walk of each loaded chunk's short list of heaps, no
/// route and no allocation, asked once per trip decision.
pub(crate) fn nearest_heap(ctx: &GoalCtx<'_>) -> Option<Point> {
    let here = ctx.body.center_position();
    // In the district, nearest the bin, where every trip starts — whether it
    // is standing there or at the fridge it got ready at.
    let from = match ctx.job {
        Some(job) if ctx.think.map.contains(here) => job.workplace,
        _ => here,
    };
    let now = ctx.think.clock.elapsed();
    let places = ctx.memory.out_of_reach();
    let key = ctx.body.home();
    let map = ctx.think.map;
    let navigation = map.navigation();
    let mut best: Option<(i32, Point)> = None;
    map.heaps(map.navigation_area(), |cell, _| {
        let candidate = (from.manhattan_distance(cell) as i32, cell);
        if best.is_some_and(|best| best <= candidate) || places.contains(cell, now) {
            return;
        }
        let reachable = cell
            .cardinal_neighbours()
            .into_iter()
            .any(|side| ctx.think.is_passable_with(side, key) && navigation.may_reach(here, side));
        if reachable {
            best = Some(candidate);
        }
    });
    best.map(|(_, cell)| cell)
}

/// Collect scrap for a living.
///
/// What it remembers is where it is going — a heap, or its bin — and how the
/// way there has gone. Which bin is its own is not here: that is the job's,
/// in the brain.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CollectGoal {
    seek: Seek,
}

impl CollectGoal {
    pub fn new() -> CollectGoal {
        CollectGoal::default()
    }

    /// The collector's own bin, if it is somewhere to store scrap: the job's
    /// workplace, and a bin there.
    fn bin(ctx: &GoalCtx<'_>) -> Option<Point> {
        let bin = ctx.job?.workplace;
        ctx.think.features.has(FeatureKind::ScrapBin, bin).then_some(bin)
    }

    /// Whether `target` is somewhere this goal still has reason to go: its
    /// own bin, or a heap with something left in it.
    fn still_worth_it(ctx: &GoalCtx<'_>, target: Point) -> bool {
        match ctx.job {
            Some(job) if job.workplace == target => Self::bin(ctx).is_some(),
            _ => ctx.think.map.scrap(target).is_some(),
        }
    }

    /// Queue the walk beside the target (unless already there) and `then`,
    /// if anything, on the back of the queue. `false` with no target, or
    /// when nowhere beside it can be stood on.
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

    /// The work to do once beside the target: salvage a heap; at the bin,
    /// store what there is — or nothing, at the start of a trip.
    fn work_at(&self, ctx: &GoalCtx<'_>) -> Option<Task> {
        let target = self.seek.target?;
        if Some(target) == ctx.job.map(|job| job.workplace) {
            let carrying = ctx.inventory.is_some_and(|pack| pack.count(ItemKind::Scrap) > 0);
            carrying.then_some(Task::deposit(target, DEPOSIT_SECONDS))
        } else {
            Some(Task::salvage_for_work(target, SALVAGE_SECONDS))
        }
    }

    /// Head for `place`, unless already on the way.
    fn head_for(&mut self, place: Point) {
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
    }

    fn forget(&mut self) {
        self.seek.forget();
    }

    /// Whether `ctx`'s unit stands beside `bin`, where every trip starts.
    fn is_beside(ctx: &GoalCtx<'_>, bin: Point) -> bool {
        ctx.body.center_position().manhattan_distance(bin) <= 1
    }
}

impl GoalExecutor for CollectGoal {
    fn goal(&self) -> GoalId {
        GoalId::Collect
    }

    fn save(&self) -> pb::Executor {
        let CollectGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: the half-walked route is gone, and so is a target
    /// that is no longer worth going to — a heap emptied meanwhile, or a bin
    /// that is not this collector's any more (let go and hired at another:
    /// `WorkGoal`'s lesson). [`CollectGoal::process`] plans the rest from
    /// where it stands, and checks the heap and the job again itself, so
    /// this is the belt to its braces: nothing stale survives either way.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        match self.seek.target {
            Some(target) if Self::still_worth_it(ctx, target) => self.seek.steady(),
            Some(_) => self.forget(),
            None => {}
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(job), Some(inventory)) = (ctx.biology, ctx.job, ctx.inventory) else {
            // Nothing to tire, nothing to do, or nothing to carry it in.
            return GoalProgress::Blocked;
        };
        let bin = job.workplace;

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // next nearest heap — but there is no other bin that is this
                // one's, so a bin out of reach gives the trip up.
                let to_the_bin = self.seek.target == Some(bin);
                let retry = if to_the_bin {
                    self.seek.setback(ctx, |_| None)
                } else {
                    self.seek.setback(ctx, nearest_heap)
                };
                if retry && self.plan(ctx, self.work_at(ctx)) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            // The work stopped short — the heap emptied under it, the pack
            // filled, the bin went, or it was pulled away: worked out again
            // below, from what is true now. Cheap.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Deposit(_))) => {
                // A packful in the bin: one trip done.
                self.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: what next, from the pack and the heaps.
        let carrying = inventory.count(ItemKind::Scrap) > 0;
        if inventory.room_for(ItemKind::Scrap) {
            if !carrying
                && !ctx.ready_to_go_out
                && ctx.think.map.contains(ctx.body.center_position())
                && !Self::is_beside(ctx, bin)
                && let Some(bin) = Self::bin(ctx)
                && nearest_heap(ctx).is_some()
            {
                // A trip starts from the bin, and is got ready for there
                // (`ReadyRoutine`): not across the district at home, a long
                // walk from the way out. Ready — got ready at the fridge
                // beside the warehouse, or called away on the way out — it
                // goes on to the heap nearest the bin from wherever it is.
                self.head_for(bin);
                if self.plan(ctx, None) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            let working = self.seek.target.filter(|&heap| heap != bin && ctx.think.map.scrap(heap).is_some());
            if let Some(heap) = working.or_else(|| nearest_heap(ctx)) {
                self.head_for(heap);
                if self.plan(ctx, Some(Task::salvage_for_work(heap, SALVAGE_SECONDS))) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            if !carrying {
                // Every heap in reach is empty: nothing to collect. Held off,
                // and the hold doubling is what spaces these lines out.
                self.forget();
                ctx.think.log.push(format!("{} found no scrap left to collect", ctx.body.uid()));
                return GoalProgress::Blocked;
            }
        }

        // A full pack, or no heap left and something to show for the trip:
        // to the bin with it.
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
    use crate::sim::job::Job;
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

    /// Where the collector's bin is on the yard below.
    const BIN: Point = Point::new(4, 3);

    /// A 9x7 site with a scrap bin on it, in the wasteland of seed 0 —
    /// `qa/scrap.json`'s, with a yard of heaps off to the west — at `clock`,
    /// with the navigation area loaded as a `GameState` has it.
    fn yard(clock: Clock) -> World {
        yard_with_bins(&[BIN], clock)
    }

    /// [`yard`], with bins in `bins`.
    fn yard_with_bins(bins: &[Point], clock: Clock) -> World {
        let mut map = Map::new(Size::new(9, 7), FLOOR).with_wasteland(0);
        for &bin in bins {
            prop_at(&mut map, "scrap bin", bin);
        }
        map.load_chunks(map.navigation_area());
        let mut world = World::new(map);
        world.clock = clock;
        world
    }

    /// A collector storing at `bin` with nothing else on its mind: every
    /// need satisfied and every process but tiring switched off.
    fn collector(cell: Point, bin: Point) -> Human {
        let mut human = needy_human(cell, 0.0, 0.0, 0.0);
        human.set_stamina(100.0);
        let biology = human.biology_mut().unwrap();
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
            biology.set_running(process, false);
        }
        assert!(human.hire(Job::collector(bin)));
        human
    }

    fn step(world: &mut World, human: &mut Human) {
        world.step_in_time(human);
    }

    fn salvaged(world: &World) -> usize {
        world.lines().iter().filter(|line| line.contains("salvaged scrap metal")).count()
    }

    #[test]
    fn a_collector_sets_off_at_half_past_eight_fills_its_pack_and_stores_it_in_its_bin() {
        let mut world = yard(at(1, 8.4));
        let mut human = collector(Point::new(2, 2), BIN);
        for _ in 0..ticks(&world, 0.05) {
            step(&mut world, &mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Collect, "twenty past eight: not yet");

        world.clock = at(1, 8.55);
        let mut out_there = false;
        for _ in 0..ticks(&world, 4.0) {
            step(&mut world, &mut human);
            out_there |= !world.map.contains(human.center_position());
            if world.deposits() > 0 {
                break;
            }
        }
        assert!(out_there, "never left the site; brain {:?}", human.brain_fields());
        assert_eq!(salvaged(&world), 3, "{:?}", world.lines());
        assert!(world.log_contains("stored 3 scrap metal in the bin at 4, 3: 3 stored"), "{:?}", world.lines());
        assert_eq!(world.bins.stored(BIN), 3);
        assert_eq!(human.inventory_mut().count(ItemKind::Scrap), 0, "the pack went in the bin");
        assert_eq!(human.center_position().manhattan_distance(BIN), 1, "beside the bin, not on it");
    }

    /// Collecting tires like office work: more than the same hours awake.
    #[test]
    fn a_trip_out_for_scrap_tires_more_than_the_hours_awake_alone() {
        let mut world = yard(at(1, 9.0));
        let mut human = collector(Point::new(2, 2), BIN);
        let start = world.clock.elapsed();
        for _ in 0..ticks(&world, 4.0) {
            step(&mut world, &mut human);
            if world.deposits() > 0 {
                break;
            }
        }
        assert_eq!(world.deposits(), 1);
        let hours = (world.clock.elapsed() - start) as f32;
        let awake = hours * crate::sim::biology::energy::ENERGY_PER_SECOND;
        let spent = 100.0 - human.stats().stamina();
        assert!(spent > awake * 1.01, "{spent} spent in {} hours, {awake} awake alone", hours / HOUR);
    }

    /// Every heap in reach empty, and nothing carried: nothing to collect,
    /// held off rather than looked for every tick.
    #[test]
    fn with_no_scrap_anywhere_the_collector_is_held_off() {
        let mut map = Map::new(Size::new(9, 7), FLOOR);
        prop_at(&mut map, "scrap bin", BIN);
        let mut world = World::new(map);
        world.clock = at(1, 10.0);
        let mut human = collector(Point::new(2, 2), BIN);
        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..600 {
            step(&mut world, &mut human);
        }
        assert_eq!(world.deposits(), 0);
        assert_ne!(human.brain().top_goal(), GoalId::Collect);
        assert!(human.brain().goals().cooldown(GoalId::Collect) > 0, "the collecting should be held off");
        assert!(world.log_contains("found no scrap left to collect"), "{:?}", world.lines());
        let lines = world.lines().iter().filter(|line| line.contains("no scrap left")).count();
        assert!(lines < 10, "{lines} lines in 600 ticks");
    }

    /// Walled in with its bin: every heap out there is one the map's
    /// components say there is no getting to, so none is chosen and no route
    /// is asked for.
    #[test]
    fn heaps_nobody_can_reach_are_skipped_rather_than_retried_every_tick() {
        let mut map = Map::new(Size::new(9, 7), FLOOR).with_wasteland(0);
        for x in 0..9 {
            map.set_terrain(Point::new(x, 0), WALL);
            map.set_terrain(Point::new(x, 6), WALL);
        }
        for y in 0..7 {
            map.set_terrain(Point::new(0, y), WALL);
            map.set_terrain(Point::new(8, y), WALL);
        }
        prop_at(&mut map, "scrap bin", BIN);
        map.load_chunks(map.navigation_area());
        let mut world = World::new(map);
        world.clock = at(1, 10.0);
        let mut human = collector(Point::new(2, 2), BIN);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..1000 {
            step(&mut world, &mut human);
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert_eq!(salvaged(&world), 0);
        assert!(routes < 100, "{routes} routes in 1000 ticks");
        assert!(world.log_contains("found no scrap left to collect"));
    }

    /// The nearest heap remembered as out of reach is passed over for the
    /// next one.
    #[test]
    fn a_heap_remembered_as_out_of_reach_is_passed_over() {
        let mut world = yard(at(1, 10.0));
        let mut human = collector(Point::new(2, 2), BIN);
        // The heap it would go to first, from its bin: find it, forget it,
        // remember it as out of reach.
        let mut first = None;
        for _ in 0..400 {
            step(&mut world, &mut human);
            if let Some((_, value)) = human.brain_fields().into_iter().find(|(name, _)| *name == "going to")
                && value != "none"
                && value != "4, 3"
            {
                first = Some(value);
                break;
            }
        }
        let first = first.expect("set off for a heap");
        let (x, y) = first.split_once(", ").unwrap();
        let heap = Point::new(x.parse().unwrap(), y.parse().unwrap());
        assert!(world.map.scrap(heap).is_some());

        let mut human = collector(Point::new(2, 2), BIN);
        human.brain_mut().memory_mut().out_of_reach_mut().remember(heap, world.clock.elapsed());
        for _ in 0..ticks(&world, 3.0) {
            step(&mut world, &mut human);
            if salvaged(&world) > 0 {
                break;
            }
        }
        assert!(salvaged(&world) > 0, "{:?}", world.lines());
        let at_heap = format!("salvaged scrap metal at {}, {}", heap.x, heap.y);
        assert!(!world.log_contains(&at_heap), "went to the heap it remembered as out of reach");
    }

    /// A packful already: straight to the bin, no heap looked at.
    #[test]
    fn a_full_pack_goes_to_the_bin_without_looking_for_a_heap() {
        let mut world = yard(at(1, 10.0));
        let mut human = collector(Point::new(7, 5), BIN);
        while human.inventory_mut().stow(ItemKind::Scrap) {}
        let carried = human.inventory_mut().count(ItemKind::Scrap);
        assert_eq!(carried, 3);
        for _ in 0..ticks(&world, 0.5) {
            step(&mut world, &mut human);
            assert!(
                !matches!(human.brain().current_task(), Some(Task::Salvage(_))),
                "went for a heap with a full pack"
            );
            if world.deposits() > 0 {
                break;
            }
        }
        assert!(world.log_contains("stored 3 scrap metal in the bin at 4, 3"), "{:?}", world.lines());
        assert_eq!(salvaged(&world), 0);
    }

    #[test]
    fn nobody_collects_at_the_weekend_or_without_the_job() {
        for day in [6, 7] {
            let mut world = yard(at(day, 10.0));
            let mut human = collector(Point::new(2, 2), BIN);
            for _ in 0..600 {
                step(&mut world, &mut human);
                assert_ne!(human.brain().top_goal(), GoalId::Collect, "day {day}");
            }
        }
        let mut world = yard(at(1, 10.0));
        let mut human = needy_human(Point::new(2, 2), 0.0, 0.0, 0.0);
        for _ in 0..600 {
            step(&mut world, &mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Collect);
        }
        // An office worker is not a collector, bin or no bin.
        let mut human = needy_human(Point::new(2, 2), 0.0, 0.0, 0.0);
        assert!(human.hire(Job::office(BIN)));
        for _ in 0..600 {
            step(&mut world, &mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Collect);
        }
    }

    /// [`yard`] with everything to see to a need with along its east side:
    /// a fridge, a TV to watch (a computer anywhere but a home is office
    /// equipment) and a toilet.
    fn yard_to_get_ready_in(clock: Clock) -> World {
        let mut map = Map::new(Size::new(9, 7), FLOOR).with_wasteland(0);
        for (name, cell) in [("scrap bin", BIN), ("fridge", Point::new(7, 1)), ("tv", Point::new(7, 3)), ("toilet", Point::new(7, 5))] {
            prop_at(&mut map, name, cell);
        }
        map.load_chunks(map.navigation_area());
        let mut world = World::new(map);
        world.clock = clock;
        world
    }

    /// Where in the log the first line containing `needle` after line
    /// `from` is.
    fn line_after(world: &World, from: usize, needle: &str) -> Option<usize> {
        world.lines().iter().skip(from).position(|line| line.contains(needle)).map(|at| at + from)
    }

    /// The point of getting ready: a collector with every need on and none of
    /// them seen to sees to each before it leaves the site — a drink, a meal,
    /// the TV, the toilet last — and nothing calls it back in before the
    /// packful is in the bin. Then again before the next trip, which starts
    /// before the shift is out.
    #[test]
    fn a_collector_sees_to_every_need_before_it_sets_out_and_nothing_calls_it_back() {
        use crate::sim::brain::routines::READY;
        let mut world = yard_to_get_ready_in(at(1, 8.4));
        let mut human = needy_human(Point::new(2, 2), 40.0, 50.0, 40.0);
        human.set_fun(50.0);
        assert!(human.hire(Job::collector(BIN)));

        let mut set_out_with = None;
        for _ in 0..ticks(&world, 8.0) {
            step(&mut world, &mut human);
            if set_out_with.is_none() && !world.map.contains(human.center_position()) {
                set_out_with = Some(human.stats());
            }
            if salvaged(&world) > 3 {
                break;
            }
        }
        let stats = set_out_with.expect("never set out");
        // The minutes from the toilet to the edge of the site.
        let margin = 2.0;
        for (need, level) in [("hunger", stats.hunger()), ("thirst", stats.thirst()), ("bladder", stats.bladder()), ("boredom", stats.boredom())] {
            assert!(level <= READY + margin, "set out with {need} at {level}: {:?}", world.lines());
        }

        // Every need seen to before the first piece of scrap — in the list's
        // order, but a bladder past 80 (the drink's water arriving) is its
        // own routine's to put first — and none between it and the bin.
        let lines = world.lines();
        let line = |from: usize, needle: &str| {
            line_after(&world, from, needle).unwrap_or_else(|| panic!("no {needle:?} after line {from}: {lines:?}"))
        };
        let first_piece = line(0, "salvaged scrap metal");
        let stored = line(first_piece, "scrap metal in the bin");
        assert!(line(0, "is ready to go out for scrap metal") < first_piece);
        for need in ["drank at the fridge", "ate at the fridge", "watched the TV", "used the toilet"] {
            assert!(line(0, need) < first_piece, "{need:?} after setting out: {lines:?}");
            assert!(line_after(&world, first_piece, need).is_none_or(|at| at > stored), "called back for {need:?}: {lines:?}");
        }

        // The second trip is got ready for too: a trip's thirst is seen to
        // at the fridge before it, not out at the heaps.
        let (drink, ready, out) =
            (line(stored, "drank at the fridge"), line(stored, "is ready to go out"), line(stored, "salvaged scrap metal"));
        assert!(drink < ready && ready < out, "{lines:?}");
    }

    /// A trip starts from the bin: a collector across the site, with nothing
    /// in its pack and nothing to see to, goes there before the heaps — and
    /// is ready the moment it is beside it. A collector the routine says is
    /// ready already (got ready at a fridge, say) goes straight out.
    #[test]
    fn a_collector_goes_out_from_its_bin_unless_it_is_ready_already() {
        let mut world = yard(at(1, 10.0));
        let mut human = collector(Point::new(7, 5), BIN);
        let mut beside_before_out = false;
        for _ in 0..ticks(&world, 1.0) {
            step(&mut world, &mut human);
            beside_before_out |= human.center_position().manhattan_distance(BIN) <= 1;
            if !world.map.contains(human.center_position()) {
                break;
            }
        }
        assert!(!world.map.contains(human.center_position()), "never set out: {:?}", human.brain_fields());
        assert!(beside_before_out, "went out without going by its bin");
        assert!(world.log_contains("is ready to go out for scrap metal"), "{:?}", world.lines());

        // Ready, across the site: out from where it stands.
        let mut world = yard(at(1, 10.0));
        let mut human = collector(Point::new(7, 5), BIN);
        human.brain_mut().set_ready_to_go_out();
        let mut first = None;
        for _ in 0..200 {
            step(&mut world, &mut human);
            first = human.brain_fields().into_iter().find(|(name, value)| *name == "going to" && value != "none");
            if first.is_some() {
                break;
            }
        }
        let (_, first) = first.expect("set off somewhere");
        assert_ne!(first, "4, 3", "went for its bin when it was ready already");
    }

    /// Let go on the way to one bin and hired at another, carrying a
    /// packful: it goes to the new one.
    #[test]
    fn a_collector_hired_at_another_bin_stores_in_the_new_one() {
        let old = Point::new(7, 5);
        let mut world = yard_with_bins(&[BIN, old], at(1, 10.0));
        let mut human = collector(Point::new(1, 1), old);
        while human.inventory_mut().stow(ItemKind::Scrap) {}
        for _ in 0..20 {
            step(&mut world, &mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Collect, "on its way to the old bin");

        assert!(human.dismiss().is_some());
        for _ in 0..5 {
            step(&mut world, &mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Collect);
        assert!(human.hire(Job::collector(BIN)));
        for _ in 0..ticks(&world, 0.5) {
            step(&mut world, &mut human);
            if world.deposits() > 0 {
                break;
            }
        }
        assert!(world.log_contains("in the bin at 4, 3"), "{:?}", world.lines());
        assert_eq!(world.bins.stored(old), 0);
    }
}
