//! [`FarmGoal`]: a farmer's shift — its field of wheat out in the good soil,
//! sown, tended and harvested, and the wheat stored in its own grain bin.
//!
//! **A field is the farm's** (`sim::fields`): the crops sown for the
//! farmer's workplace, its grain bin, up to [`PLOTS`] of them. One piece of
//! work at a time, each a walk beside the place and a task there, **planned
//! from what is true now** (rule 4), in this order:
//!
//! 1. **fewer crops than plots** — the free plot of good soil nearest the
//!    field, or with none yet the farm's bin ([`nearest_plot`]):
//!    [`Sow`](crate::sim::brain::tasks::Sow) it. So a crop harvested is sown
//!    again before the next is cut;
//! 2. **a ripe crop** — the nearest: [`Harvest`](crate::sim::brain::tasks::Harvest)
//!    it, if the pack has room for what it yields; if not, to the bin first;
//! 3. **a crop not tended today** — the nearest:
//!    [`Tend`](crate::sim::brain::tasks::Tend) it;
//! 4. **wheat in the pack** — to the bin, and
//!    [`Deposit`](crate::sim::brain::tasks::Deposit) it: the goal is
//!    `Achieved`, and the routine starts the next piece of work;
//! 5. **nothing** — every crop tended, nothing ripe, nothing carried: the
//!    day's work is done, `Blocked`, and the hold spaces out the next look.
//!
//! **How long a day it is is the routine's to say**; this goal never reads
//! the clock but to ask a crop how it is ([`Crop::is_ripe`],
//! [`Crop::needs_tending`]). Wheat still in the pack at five o'clock goes
//! home with the farmer and into the bin the next working day.
//!
//! [`Crop::is_ripe`]: crate::sim::fields::Crop::is_ripe
//! [`Crop::needs_tending`]: crate::sim::fields::Crop::needs_tending

use crate::map::{is_plot, Point};
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::fields::{Crop, PLOTS};
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Seek, Stand};
use crate::sim::brain::tasks::{DEPOSIT_SECONDS, HARVEST_SECONDS, SOW_SECONDS, TEND_SECONDS};

/// Whether this unit could get beside `cell` to work it: a side it may stand
/// on that the map's components say it can reach, and `cell` not remembered
/// as out of reach. O(1) lookups, no route.
fn workable(ctx: &GoalCtx<'_>, cell: Point) -> bool {
    let here = ctx.body.center_position();
    let key = ctx.body.home();
    let navigation = ctx.think.map.navigation();
    !ctx.memory.out_of_reach().contains(cell, ctx.think.clock.elapsed())
        && cell
            .cardinal_neighbours()
            .into_iter()
            .any(|side| ctx.think.is_passable_with(side, key) && navigation.may_reach(here, side))
}

/// The crop of the farm at `farm` nearest this unit, as the crow flies, that
/// `wanted` says yes to and it could get to work. Ties go to the lower cell.
fn nearest_crop(ctx: &GoalCtx<'_>, farm: Point, wanted: impl Fn(&Crop) -> bool) -> Option<Crop> {
    let here = ctx.body.center_position();
    ctx.think
        .fields
        .of(farm)
        .filter(|crop| wanted(crop) && workable(ctx, crop.cell))
        .min_by_key(|crop| (here.manhattan_distance(crop.cell), crop.cell))
        .copied()
}

/// The free plot of good soil for the farm at `farm` to sow next: the one
/// nearest its field — the field's crop nearest the farm's grain bin, or
/// with no field yet the bin itself — that this unit could get to, with
/// nobody in it, alive or dead, and nobody else beside it (somebody there is
/// most likely sowing it). Ring by ring out from there, stopping once no ring further
/// out could hold a nearer one; **within the navigation area** (the site and
/// [`NAV_MARGIN`](crate::map::NAV_MARGIN) round it), which a `GameState`
/// always has loaded and reachability is worked out over. No route and no
/// allocation; asked once a crop is to be sown.
///
/// So a field grows round its first crop, the farm's fields together a field
/// round the farm, and a crop harvested is sown again where it stood.
pub(crate) fn nearest_plot(ctx: &GoalCtx<'_>, farm: Point) -> Option<Point> {
    let fields = ctx.think.fields;
    let from = fields.of(farm).map(|crop| crop.cell).min_by_key(|&cell| (cell.manhattan_distance(farm), cell)).unwrap_or(farm);
    let map = ctx.think.map;
    let area = map.navigation_area();
    let (low, high) = (area.origin, area.max());
    let reach = (from.x - low.x).max(high.x - from.x).max(from.y - low.y).max(high.y - from.y).max(0);
    let mut best: Option<(i32, Point)> = None;
    for ring in 0..=reach {
        // Manhattan is never less than Chebyshev: past here nothing is nearer.
        if best.is_some_and(|(distance, _)| distance < ring) {
            break;
        }
        let mut consider = |cell: Point| {
            if !is_plot(cell) || !area.contains(cell) {
                return;
            }
            let candidate = (from.manhattan_distance(cell), cell);
            if best.is_some_and(|best| best <= candidate) {
                return;
            }
            let somebody_at_it = || {
                ctx.think.occupancy.is_occupied(cell)
                    || ctx.think.corpses.at(cell).is_some()
                    || cell.cardinal_neighbours().into_iter().any(|side| {
                        ctx.think.occupancy.occupant(side).is_some_and(|other| other != ctx.body.uid())
                    })
            };
            if map.can_sow(cell) && !somebody_at_it() && workable(ctx, cell) {
                best = Some(candidate);
            }
        };
        if ring == 0 {
            consider(from);
            continue;
        }
        for dx in -ring..=ring {
            consider(from.offset(dx, -ring));
            consider(from.offset(dx, ring));
        }
        for dy in 1 - ring..ring {
            consider(from.offset(-ring, dy));
            consider(from.offset(ring, dy));
        }
    }
    best.map(|(_, cell)| cell)
}

/// Farm for a living.
///
/// What it remembers is where it is going — a crop, a plot, or its bin — and
/// how the way there has gone. Which bin, and so which field, is its own is
/// the job's, in the brain.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FarmGoal {
    seek: Seek,
}

impl FarmGoal {
    pub fn new() -> FarmGoal {
        FarmGoal::default()
    }

    /// The farmer's own grain bin, if it is somewhere to store wheat: the
    /// job's workplace, and a bin there.
    fn bin(ctx: &GoalCtx<'_>) -> Option<Point> {
        let bin = ctx.job?.workplace;
        ctx.think.features.has(FeatureKind::GrainBin, bin).then_some(bin)
    }

    /// What to work at next, from the field and the pack as they are: a
    /// crop, a plot, or the bin (`farm`) — or nothing to do.
    fn next(ctx: &GoalCtx<'_>, farm: Point) -> Option<Point> {
        let pack = ctx.inventory?;
        let now = ctx.think.clock;
        let carrying = pack.count(ItemKind::Wheat) > 0;
        if ctx.think.fields.of(farm).count() < PLOTS
            && let Some(plot) = nearest_plot(ctx, farm)
        {
            return Some(plot);
        }
        if let Some(ripe) = nearest_crop(ctx, farm, |crop| crop.is_ripe(now)) {
            if pack.room_for_many(ItemKind::Wheat, ripe.harvest()) {
                return Some(ripe.cell);
            }
            if carrying {
                return Some(farm);
            }
        }
        if let Some(crop) = nearest_crop(ctx, farm, |crop| crop.needs_tending(now)) {
            return Some(crop.cell);
        }
        carrying.then_some(farm)
    }

    /// The work to do at `target`, from beside it, as it is now — `None`
    /// where there is none left.
    fn work_at(ctx: &GoalCtx<'_>, farm: Point, target: Point) -> Option<Task> {
        if target == farm {
            let carrying = ctx.inventory.is_some_and(|pack| pack.count(ItemKind::Wheat) > 0);
            return (carrying && Self::bin(ctx).is_some()).then_some(Task::deposit(farm, DEPOSIT_SECONDS));
        }
        let now = ctx.think.clock;
        match ctx.think.fields.at(target) {
            Some(crop) if crop.farm != farm => None,
            Some(crop) if crop.is_ripe(now) => Some(Task::harvest(target, HARVEST_SECONDS)),
            Some(crop) if crop.needs_tending(now) => Some(Task::tend(target, TEND_SECONDS)),
            Some(_) => None,
            None => (ctx.think.fields.of(farm).count() < PLOTS && ctx.think.map.can_sow(target)).then_some(Task::sow(target, SOW_SECONDS)),
        }
    }

    /// Queue the walk beside the target (unless already there) and `then`,
    /// on the back of the queue. `false` when nowhere beside it can be stood
    /// on.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>, then: Task) -> bool {
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
        let _ = ctx.tasks.push_back(then);
        true
    }

    /// Head for `place`, unless already on the way.
    fn head_for(&mut self, place: Point) {
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
    }
}

impl GoalExecutor for FarmGoal {
    fn goal(&self) -> GoalId {
        GoalId::Farm
    }

    fn save(&self) -> pb::Executor {
        let FarmGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: the half-walked route is gone, and so is a target
    /// that has no work left in it — a crop tended or harvested meanwhile, a
    /// plot somebody else sowed, a bin that is not this farmer's any more.
    /// [`FarmGoal::process`] plans the rest from where it stands.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        let worth_it = match (self.seek.target, ctx.job) {
            (Some(target), Some(job)) => Self::work_at(ctx, job.workplace, target).is_some(),
            _ => false,
        };
        if worth_it {
            self.seek.steady();
        } else {
            self.seek.forget();
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(_), Some(farm)) = (ctx.biology, ctx.inventory, Self::bin(ctx)) else {
            // Nothing to tire, nothing to carry the wheat in, or no farm: a
            // field is sown for a grain bin, and there is none.
            return GoalProgress::Blocked;
        };

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // next thing to do in the field — but there is no other bin
                // that is this one's, so a bin out of reach gives it up.
                let retry = if self.seek.target == Some(farm) {
                    self.seek.setback(ctx, |_| None)
                } else {
                    self.seek.setback(ctx, |ctx| Self::next(ctx, farm).filter(|&next| next != farm))
                };
                if retry
                    && let Some(target) = self.seek.target
                    && let Some(then) = Self::work_at(ctx, farm, target)
                    && self.plan(ctx, then)
                {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            // The work stopped short — the crop went, the plot was sown under
            // it, somebody stood in it, or it was pulled away: worked out
            // again below. Cheap.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Deposit(_))) => {
                // The wheat in the bin: a piece of work done.
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: what next, from the field and the pack. Nothing at
        // all is the day's work done, most afternoons — not worth a line in
        // the log, unlike a collector finding the scrap gone.
        let Some(target) = Self::next(ctx, farm) else {
            self.seek.forget();
            return GoalProgress::Blocked;
        };
        let Some(then) = Self::work_at(ctx, farm, target) else {
            self.seek.forget();
            return GoalProgress::Blocked;
        };
        self.head_for(target);
        if self.plan(ctx, then) {
            GoalProgress::Working
        } else {
            self.seek.forget();
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
    use crate::map::{Map, Size, MEADOW};
    use crate::sim::biology::ProcessId;
    use crate::sim::clock::{Clock, DAY, HOUR, TIME_SCALE};
    use crate::sim::fields::{Stage, BEST, MOST_YIELDED, TEND_QUALITY};
    use crate::sim::job::{Job, JobKind, FARM_HOURS};
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

    /// The farm's grain bin on the meadow below.
    const BIN: Point = Point::new(1, 7);

    /// A site of good soil, 24 by 14, with a grain bin on it, at `clock`.
    fn meadow(clock: Clock) -> World {
        let mut map = Map::new(Size::new(24, 14), MEADOW);
        prop_at(&mut map, "grain bin", BIN);
        let mut world = World::new(map);
        world.clock = clock;
        world
    }

    /// A farmer of the bin at [`BIN`] with nothing else on its mind: every
    /// need satisfied and every process switched off, so this is about the
    /// job and a week of it is a week.
    fn farmer(cell: Point) -> Human {
        let mut human = needy_human(cell, 0.0, 0.0, 0.0);
        human.set_stamina(100.0);
        let biology = human.biology_mut().unwrap();
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun, ProcessId::Energy] {
            biology.set_running(process, false);
        }
        assert!(human.hire(Job::new(JobKind::Farmer, BIN, FARM_HOURS)));
        human
    }

    /// Day `day` from half past eight, setting off, to five, the shift's end.
    fn work_the_day(world: &mut World, human: &mut Human, day: u32) {
        world.clock = at(day, 8.5);
        for _ in 0..ticks(world, 8.5) {
            world.step_in_time(human);
            assert!(world.fields.at(human.center_position()).is_none(), "standing in a crop at {:?}", human.center_position());
        }
    }

    fn lines_with(world: &World, needle: &str) -> usize {
        world.lines().iter().filter(|line| line.contains(needle)).count()
    }

    /// The Monday: ten crops, every one in a plot, every one for this farm,
    /// and the field the plots nearest the farm's bin.
    #[test]
    fn on_the_monday_a_farmer_sows_a_field_of_ten_beside_the_farm() {
        let mut world = meadow(at(1, 8.5));
        let mut human = farmer(Point::new(12, 3));
        work_the_day(&mut world, &mut human, 1);
        let field: Vec<Crop> = world.fields.of(BIN).copied().collect();
        assert_eq!(field.len(), PLOTS, "{:?}", world.lines());
        assert_eq!(world.fields.len(), PLOTS, "and none for anybody else");
        assert_eq!(lines_with(&world, "sowed wheat at"), PLOTS);
        assert!(world.log_contains(&format!("{PLOTS} of {PLOTS} sown")));
        // The first crop in the plot nearest the bin that could be sown — not
        // the ones beside the bin, with the bin in the ring round them — and
        // the field round it.
        let first = field.iter().map(|crop| crop.cell).min_by_key(|&cell| (cell.manhattan_distance(BIN), cell)).unwrap();
        let fresh = meadow(at(1, 8.5));
        let plots: Vec<Point> = fresh.map.size().points().filter(|&cell| fresh.map.can_sow(cell)).collect();
        assert!(!plots.contains(&Point::new(2, 6)) && !plots.contains(&Point::new(2, 8)), "the bin is beside them");
        assert!(plots.iter().all(|plot| plot.manhattan_distance(BIN) >= first.manhattan_distance(BIN)));
        let farthest = field.iter().map(|crop| crop.cell.manhattan_distance(first)).max().unwrap();
        for &plot in plots.iter().filter(|&&cell| world.map.can_sow(cell)) {
            assert!(plot.manhattan_distance(first) >= farthest, "{plot:?} is nearer the field than its farthest crop");
        }
        for crop in &field {
            assert!(is_plot(crop.cell) && !world.map.is_passable(crop.cell));
            assert_eq!((crop.stage(world.clock), crop.quality, crop.tended), (Stage::Seedling, 0, 1));
        }
        assert!(!world.log_contains("tended the wheat"), "nothing is tended the day it is sown");
    }

    /// Every working day after, the whole field tended once; the Sunday off,
    /// when it ripens; the next Monday harvested at its best, every sheaf in
    /// the bin, and the field sown again.
    #[test]
    fn a_week_on_the_farm_tends_the_field_every_day_but_sunday_and_brings_in_the_harvest() {
        let mut world = meadow(at(1, 8.5));
        let mut human = farmer(Point::new(12, 3));
        work_the_day(&mut world, &mut human, 1);
        for day in 2..=6 {
            work_the_day(&mut world, &mut human, day);
            for crop in world.fields.of(BIN) {
                assert_eq!((crop.tended, crop.quality), (day, TEND_QUALITY * (day as u8 - 1)), "day {day}: {crop:?}");
            }
        }
        assert!(world.fields.of(BIN).all(|crop| crop.quality == BEST));
        let tended = lines_with(&world, "tended the wheat");
        assert_eq!(tended, 5 * PLOTS);

        // Sunday: the day off, and the field ripens.
        world.clock = at(7, 8.5);
        for _ in 0..ticks(&world, 8.5) {
            world.step_in_time(&mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Farm, "a farmer's Sunday is its own");
        }
        assert_eq!(lines_with(&world, "tended the wheat"), tended);
        assert!(world.fields.of(BIN).all(|crop| crop.is_ripe(world.clock)), "ripe six days after sowing");

        // Monday: in with the harvest, and the field sown again.
        work_the_day(&mut world, &mut human, 8);
        assert_eq!(lines_with(&world, "harvested 3 wheat at"), PLOTS, "{:?}", world.lines());
        assert_eq!(world.bins.stored(BIN), PLOTS as u32 * MOST_YIELDED as u32, "{:?}", world.lines());
        assert!(lines_with(&world, "wheat in the grain bin at 1, 7") >= 3, "a pack holds four crops' worth");
        assert_eq!(human.inventory_mut().count(ItemKind::Wheat), 0);
        let field: Vec<Crop> = world.fields.of(BIN).copied().collect();
        assert_eq!(field.len(), PLOTS);
        assert!(field.iter().all(|crop| crop.tended == 8 && crop.quality == 0), "sown again: {field:?}");
    }

    /// The day's tending done by midday: nothing to do, held off rather than
    /// looked for every tick, and the log not filled saying so.
    #[test]
    fn with_the_field_tended_the_farmer_is_held_off_till_the_next_day() {
        let mut world = meadow(at(1, 8.5));
        let mut human = farmer(Point::new(12, 3));
        work_the_day(&mut world, &mut human, 1);
        world.clock = at(2, 8.5);
        let mut held_off = 0;
        for _ in 0..ticks(&world, 8.5) {
            world.step_in_time(&mut human);
            if human.brain().goals().cooldown(GoalId::Farm) > 0 {
                held_off += 1;
            }
        }
        assert!(world.fields.of(BIN).all(|crop| crop.tended == 2), "the day's work done");
        assert!(held_off > ticks(&world, 3.0), "held off only {held_off} ticks of the afternoon");
        assert!(world.lines().len() < 3 * PLOTS, "{} lines", world.lines().len());
    }

    /// A farmer whose grain bin is gone has no farm to sow a field for.
    #[test]
    fn with_no_grain_bin_there_is_no_field() {
        let mut world = World::new(Map::new(Size::new(24, 14), MEADOW));
        world.clock = at(1, 8.5);
        let mut human = farmer(Point::new(12, 3));
        work_the_day(&mut world, &mut human, 1);
        assert!(world.fields.is_empty());
    }
}
