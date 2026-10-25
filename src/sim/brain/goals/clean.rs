//! [`CleanGoal`]: a cleaner's shift — look over the district for dirt, mop
//! it up a cell at a time, nearest first, and with none to find walk round.
//!
//! **The look is the cleaner's own, and it is periodic**: every
//! [`SCAN_EVERY`] (27 world minutes) of a shift, on the tick it is due
//! whatever the cleaner is doing (`GoalExecutor::keep_up`, which may cut a
//! walk short for something nearer), its brain reads the whole of the site's
//! dirt layer
//! ([`DirtLayer::site_stains`](crate::sim::dirt::DirtLayer::site_stains)) and
//! keeps the [`FOUND`] nearest places worth cleaning — dirtier than [`DIRTY`]
//! all told, nobody's home, and somewhere it can get to (the map's
//! components, no route asked). A read of a few thousand cells and an
//! insertion into an array kept in the goal: no allocation, no scan of the
//! crowd, and on duty only — off shift the goal is not processed at all.
//! Between looks it works through what it found, the nearest to where it
//! stands each time, checking each cell again as it gets to it (somebody
//! else may have mopped it, or it dried).
//!
//! **There is nowhere it waits.** Its workplace is the district
//! ([`FeatureKind::District`](crate::sim::feature::FeatureKind::District)):
//! with nothing found, or everything found cleaned, it walks to somewhere
//! public within [`ROAM_RADIUS`] and looks again when the next look is due.

use rand::RngExt;
use rand::rngs::SmallRng;

use crate::map::Point;
use crate::save::{Invalid, invalid, pb};
use crate::sim::JobKind;
use crate::sim::clock::MINUTE;
use crate::sim::rng::tick_rng;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::clean::CLEAN_SECONDS;
use super::wander::WANDER_TRIES;
use super::{Seek, Stand, stand_beside};

/// How dirty a cell has to be, its four contaminants summed
/// ([`Dirt::overall`](crate::sim::dirt::Dirt::overall)), for a cleaner to go
/// and clean it. A trail's faint end is left to fade.
pub const DIRTY: f32 = 0.15;

/// How often an on-duty cleaner looks over the district, in **world**
/// seconds: every 27 minutes.
pub const SCAN_EVERY: f64 = 27.0 * MINUTE as f64;

/// How many places one look remembers, the nearest it found.
pub const FOUND: usize = 16;

/// How far a cleaner with nothing to clean walks, in cells.
pub const ROAM_RADIUS: i32 = 12;

/// See the [module docs](self). Inline and `Copy`, as every goal is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CleanGoal {
    /// The cell being walked to or mopped.
    seek: Seek,
    /// What the last look found and is not cleaned yet, nearest first (as
    /// of the look); `None` past the end.
    found: [Option<Point>; FOUND],
    /// When it last looked, in world seconds since opening time; `None`
    /// before its first look.
    scanned: Option<OrderedSeconds>,
    /// Where it is walking with nothing to clean.
    roam: Option<Point>,
}

/// World seconds, compared by their bits: a goal is `Eq`, and the clock
/// never reads NaN.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct OrderedSeconds(u64);

impl OrderedSeconds {
    fn new(seconds: f64) -> Self {
        Self(seconds.to_bits())
    }

    fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// Whether `cell` is worth going to clean now: dirty enough, on the site,
/// nobody's home, not lately given up on, and with somewhere to stand.
fn worth_cleaning(ctx: &GoalCtx<'_>, cell: Point) -> bool {
    ctx.think.map.site().contains(cell)
        && ctx.think.dirt.at(cell).overall() > DIRTY
        && ctx.think.properties.of(cell).is_none()
        && !ctx.memory.out_of_reach().contains(cell, ctx.think.clock.elapsed())
        && !matches!(stand_beside(ctx, cell), Stand::Nowhere)
}

impl CleanGoal {
    pub fn new() -> CleanGoal {
        CleanGoal::default()
    }

    /// Look over the district: the [`FOUND`] places nearest to where it
    /// stands that are worth cleaning and that it can get to, nearest first,
    /// ties to the lower cell. How many there were in all, for the log.
    fn scan(&mut self, ctx: &GoalCtx<'_>) -> usize {
        let here = ctx.body.center_position();
        let navigation = ctx.think.map.navigation();
        let now = ctx.think.clock.elapsed();
        let mut found: [Option<(i32, Point)>; FOUND] = [None; FOUND];
        let mut dirty = 0;
        for (cell, stain) in ctx.think.dirt.site_stains() {
            if stain.overall() <= DIRTY
                || ctx.think.properties.of(cell).is_some()
                || !navigation.may_reach(here, cell)
                || ctx.memory.out_of_reach().contains(cell, now)
            {
                continue;
            }
            dirty += 1;
            let mut entry = Some((here.manhattan_distance(cell), cell));
            for slot in &mut found {
                // Insertion into a sorted array: `None` sorts last.
                if slot.is_none_or(|kept| entry.is_some_and(|new| new < kept)) {
                    std::mem::swap(slot, &mut entry);
                }
                if entry.is_none() {
                    break;
                }
            }
        }
        self.found = found.map(|slot| slot.map(|(_, cell)| cell));
        dirty
    }

    /// Whether [`SCAN_EVERY`] has gone by since the last look, or there has
    /// been none.
    fn is_look_due(&self, ctx: &GoalCtx<'_>) -> bool {
        self.scanned.is_none_or(|at| ctx.think.clock.elapsed() - at.get() >= SCAN_EVERY)
    }

    /// Look over the district now ([`CleanGoal::scan`]), and say so.
    fn look(&mut self, ctx: &GoalCtx<'_>) {
        let dirty = self.scan(ctx);
        self.scanned = Some(OrderedSeconds::new(ctx.think.clock.elapsed()));
        let place = if dirty == 1 { "place" } else { "places" };
        ctx.think.log.unit(ctx.body.uid(), format!("{} looked over the district: {dirty} dirty {place}", ctx.body.uid()));
    }

    /// The found place nearest to where it stands now, taken off the list.
    fn take_nearest(&mut self, here: Point) -> Option<Point> {
        let index = (0..FOUND)
            .filter_map(|i| self.found[i].map(|cell| (here.manhattan_distance(cell), cell, i)))
            .min()?
            .2;
        let cell = self.found[index].take();
        // Close the gap, so the list stays nearest first and `None` last.
        self.found[index..].rotate_left(1);
        cell
    }

    /// Queue the walk beside the target, if it is not within a step, and the
    /// mopping. `false` with nowhere to stand.
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
        let _ = ctx.tasks.push_back(Task::clean(target, CLEAN_SECONDS));
        true
    }

    /// Somewhere public on the site within [`ROAM_RADIUS`] that it can get
    /// to, or `None` if a few tries found nothing — a cheap miss, as a
    /// wanderer's is.
    fn pick_roam(ctx: &GoalCtx<'_>, rng: &mut SmallRng) -> Option<Point> {
        let here = ctx.body.center_position();
        let navigation = ctx.think.map.navigation();
        let site = ctx.think.map.site();
        for _ in 0..WANDER_TRIES {
            let candidate = Point::new(
                here.x + rng.random_range(-ROAM_RADIUS..=ROAM_RADIUS),
                here.y + rng.random_range(-ROAM_RADIUS..=ROAM_RADIUS),
            );
            if candidate != here
                && site.contains(candidate)
                && ctx.think.is_passable(candidate)
                && ctx.think.properties.of(candidate).is_none()
                && navigation.may_reach(here, candidate)
            {
                return Some(candidate);
            }
        }
        None
    }

    fn debug_found(&self) -> String {
        let cells: Vec<String> = self.found.iter().flatten().map(|cell| format!("{}, {}", cell.x, cell.y)).collect();
        if cells.is_empty() { "none".to_string() } else { cells.join("; ") }
    }
}

impl GoalExecutor for CleanGoal {
    fn goal(&self) -> GoalId {
        GoalId::Clean
    }

    fn save(&self) -> pb::Executor {
        let CleanGoal { seek, found, scanned, roam } = *self;
        pb::Executor::Clean(pb::Clean {
            seek: Some(seek.save()),
            found: found.iter().flatten().map(|&cell| cell.into()).collect(),
            scanned: scanned.map(OrderedSeconds::get),
            roam: roam.map(Into::into),
        })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Clean(clean) = saved else {
            return invalid("cleaning is a seek, what was found, when it looked and where it roams");
        };
        if clean.found.len() > FOUND {
            return invalid(format!("clean: {} places found, at most {FOUND}", clean.found.len()));
        }
        if clean.scanned.is_some_and(|at| !at.is_finite()) {
            return invalid("clean: looked at no time");
        }
        let mut found = [None; FOUND];
        for (slot, &cell) in found.iter_mut().zip(&clean.found) {
            *slot = Some(cell.into());
        }
        *self = CleanGoal {
            seek: Seek::restore(&clean.seek.unwrap_or_default())?,
            found,
            scanned: clean.scanned.map(OrderedSeconds::new),
            roam: clean.roam.map(Into::into),
        };
        Ok(())
    }

    /// Carry on with the cell it was going to clean if it is still worth it;
    /// what it found and when it looked are kept. A walk round is not.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        self.roam = None;
        if self.seek.target.is_some_and(|cell| worth_cleaning(ctx, cell)) {
            self.seek.steady();
        } else {
            self.seek.forget();
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() || !ctx.job.is_some_and(|job| job.kind == JobKind::Cleaner) {
            return GoalProgress::Blocked;
        }
        match (last, ctx.finished) {
            (TaskResult::Success, Some(Task::Clean(_))) => {
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Failed, Some(Task::MoveTo(_))) if self.roam.take().is_some() => {
                ctx.tasks.clear();
                // A body in the way of a walk round: somewhere else. No way
                // there at all: the expensive miss.
                if ctx.blocked_by.is_none() {
                    return GoalProgress::Blocked;
                }
            }
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                if self.seek.setback(ctx, |_| None) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
            }
            (TaskResult::Failed, _) => {
                // Cleaned under it, or pulled away: the next place.
                ctx.tasks.clear();
                self.seek.forget();
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => {
                self.roam = None;
                self.seek.steady();
            }
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        if self.is_look_due(ctx) {
            self.look(ctx);
        }

        // Back to the cell it was on before a break, or the nearest found.
        if self.seek.target.is_some_and(|cell| worth_cleaning(ctx, cell)) && self.plan(ctx) {
            return GoalProgress::Working;
        }
        self.seek.forget();
        let here = ctx.body.center_position();
        while let Some(cell) = self.take_nearest(here) {
            if worth_cleaning(ctx, cell) {
                self.seek.choose(cell);
                if self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
            }
        }

        // Nothing to clean: a walk round, until the next look.
        let mut rng = tick_rng(ctx.body.uid(), ctx.think.tick);
        if let Some(target) = Self::pick_roam(ctx, &mut rng) {
            self.roam = Some(target);
            let _ = ctx.tasks.push_back(Task::move_to(target));
        }
        GoalProgress::Working
    }

    /// The look over the district, on the tick it is due, whatever the
    /// cleaner is doing. A walk is cut short for what it found nearer than
    /// where it was going — anything, from a walk round; a cell being mopped
    /// is mopped to the end, and the next is chosen from the fresh look.
    fn keep_up(&mut self, ctx: &mut GoalCtx<'_>, current: &Task) -> bool {
        if !self.is_look_due(ctx) || !ctx.job.is_some_and(|job| job.kind == JobKind::Cleaner) {
            return false;
        }
        self.look(ctx);
        if !matches!(current, Task::MoveTo(_)) {
            return false;
        }
        let here = ctx.body.center_position();
        let going = self.seek.target.map_or(i32::MAX, |target| here.manhattan_distance(target));
        let nearer = self.found.iter().flatten().any(|&cell| here.manhattan_distance(cell) < going);
        if nearer {
            self.seek.forget();
            self.roam = None;
        }
        nearer
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let cell =|cell: Option<Point>| cell.map_or_else(|| "none".to_string(), |cell| format!("{}, {}", cell.x, cell.y));
        vec![
            ("cleaning", cell(self.seek.target)),
            ("found", self.debug_found()),
            ("looked", self.scanned.map_or_else(|| "never".to_string(), |at| format!("{:.0} s", at.get()))),
            ("roaming to", cell(self.roam)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Map, Size, WALL};
    use crate::sim::biology::ProcessId;
    use crate::sim::dirt::Dirt;
    use crate::sim::job::{CLEANER_SHIFTS, Job, Shift};
    use crate::sim::testing::{World, needy_human, prop_at};
    use crate::sim::{GameEntity, Human};

    /// A street 24 by 12 with a house in its corner: walls round 16..=21 by
    /// 1..=4, its door in the bottom wall at 18, 5.
    fn street() -> Map {
        let mut map = Map::new(Size::new(24, 12), FLOOR);
        for x in 15..=22 {
            map.set_terrain(Point::new(x, 0), WALL);
            map.set_terrain(Point::new(x, 5), WALL);
        }
        for y in 0..=5 {
            map.set_terrain(Point::new(15, y), WALL);
            map.set_terrain(Point::new(22, y), WALL);
        }
        let door = Point::new(18, 5);
        map.set_terrain(door, FLOOR);
        prop_at(&mut map, crate::sim::door::HOUSE_DOOR, door);
        map
    }

    const HOME: Point = Point::new(18, 2);

    /// A cleaner on `shift` with no needs to see to, standing at 3, 8. The
    /// clock is at opening time: eight on the Monday.
    fn cleaner(shift: Shift) -> Human {
        let mut human = needy_human(Point::new(3, 8), 0.0, 0.0, 0.0);
        for process in ProcessId::ALL {
            human.biology_mut().unwrap().set_running(process, false);
        }
        assert!(human.hire(Job::new(JobKind::Cleaner, Point::new(1, 1), shift)));
        human
    }

    fn stain(world: &mut World, cell: Point, dirt: f32) {
        assert!(world.facilities.dirt.set(cell, Dirt { dirt, blood: dirt / 2.0, ..Dirt::CLEAN }));
    }

    #[test]
    fn a_cleaner_on_duty_mops_up_public_ground_and_leaves_homes_and_faint_trails_alone() {
        let mut world = World::new(street());
        assert!(world.properties.of(HOME).is_some(), "the house is somebody's");
        stain(&mut world, Point::new(9, 9), 0.3);
        stain(&mut world, Point::new(4, 2), 0.2);
        stain(&mut world, Point::new(11, 3), 0.08);
        stain(&mut world, HOME, 0.6);
        let mut human = cleaner(CLEANER_SHIFTS[0]);
        for _ in 0..3000 {
            world.step_in_time(&mut human);
        }
        assert_eq!(world.dirt.at(Point::new(9, 9)), Dirt::CLEAN, "{:?}", world.lines());
        assert_eq!(world.dirt.at(Point::new(4, 2)), Dirt::CLEAN);
        assert!(world.log_contains("looked over the district: 2 dirty places"), "{:?}", world.lines());
        assert!(world.log_contains("cleaned up dirt and blood at 9, 9"));
        assert!(!world.log_contains("at 11, 3"), "a faint trail is left to fade");
        assert!(!world.log_contains("at 18, 2"), "nobody's home is cleaned");
        assert_eq!(world.dirt.at(HOME).dirt, 0.6, "nor walked into");
    }

    /// The look over the district is the cleaner's own and periodic: once
    /// every 27 world minutes while it is on duty, no oftener, and dirt that
    /// turns up between two looks waits for the second. With nothing to
    /// clean it walks round public ground, never waiting anywhere.
    #[test]
    fn a_cleaner_looks_over_the_district_every_27_minutes_and_walks_round_between() {
        let mut world = World::new(street());
        let mut human = cleaner(CLEANER_SHIFTS[0]);
        let looked = |world: &World| world.lines().iter().filter(|line| line.contains("looked over the district")).count();
        let mut looks: Vec<f64> = Vec::new();
        let mut visited = std::collections::BTreeSet::new();
        let late = Point::new(20, 9);
        let mut stained_at = None;
        let mut cleaned_at = None;
        // Three hours of world.
        for _ in 0..5760 {
            world.step_in_time(&mut human);
            if looked(&world) > looks.len() {
                looks.push(world.clock.elapsed());
            }
            if looks.len() == 1 && stained_at.is_none() && world.clock.elapsed() - looks[0] > 5.0 * MINUTE as f64 {
                stain(&mut world, late, 0.4);
                stained_at = Some(world.clock.elapsed());
            }
            if stained_at.is_some() && cleaned_at.is_none() && world.dirt.at(late) == Dirt::CLEAN {
                cleaned_at = Some(world.clock.elapsed());
            }
            let here = human.center_position();
            assert!(world.properties.of(here).is_none(), "into somebody's home at {here:?}");
            visited.insert(here);
        }
        assert!(looks.len() >= 6, "{} looks in three hours", looks.len());
        for pair in looks.windows(2) {
            let gap = pair[1] - pair[0];
            // To the tick (1.875 world seconds), whatever it was doing.
            assert!(gap >= SCAN_EVERY && gap < SCAN_EVERY + 4.0, "looked {gap} s apart");
        }
        let cleaned_at = cleaned_at.expect("the stain that turned up between looks is cleaned");
        assert!(cleaned_at > looks[1], "not before the second look");
        assert!(world.log_contains("looked over the district: 1 dirty place"));
        assert!(visited.len() >= 20, "walked round {} cells", visited.len());
    }

    #[test]
    fn off_duty_a_cleaner_neither_looks_nor_cleans() {
        let mut world = World::new(street());
        stain(&mut world, Point::new(9, 9), 0.3);
        // The second group's morning: Tuesday, Thursday, Saturday — and the
        // clock is on a Monday.
        let mut human = cleaner(CLEANER_SHIFTS[1]);
        for _ in 0..1500 {
            world.step_in_time(&mut human);
        }
        assert!(!world.log_contains("looked over the district"), "{:?}", world.lines());
        assert_ne!(world.dirt.at(Point::new(9, 9)), Dirt::CLEAN);
        assert_ne!(human.brain().top_goal(), GoalId::Clean);
    }

    #[test]
    fn a_cleaners_round_comes_back_from_a_save_as_it_was_and_nonsense_is_refused() {
        let mut found = [None; FOUND];
        found[0] = Some(Point::new(4, -2));
        found[1] = Some(Point::new(9, 9));
        let goal = CleanGoal {
            seek: Seek::at(Point::new(3, 3)),
            found,
            scanned: Some(OrderedSeconds::new(1234.5)),
            roam: Some(Point::new(-1, 7)),
        };
        let mut back = CleanGoal::new();
        back.restore(&goal.save()).unwrap();
        assert_eq!(back, goal);
        let mut fresh = CleanGoal::new();
        fresh.restore(&CleanGoal::new().save()).unwrap();
        assert_eq!(fresh, CleanGoal::new());

        let pb::Executor::Clean(mut saved) = goal.save() else { unreachable!() };
        saved.scanned = Some(f64::NAN);
        assert!(back.restore(&pb::Executor::Clean(saved.clone())).is_err(), "looked at no time");
        saved.scanned = None;
        saved.found = vec![Point::new(0, 0).into(); FOUND + 1];
        assert!(back.restore(&pb::Executor::Clean(saved)).is_err(), "more found than a look keeps");
        assert!(back.restore(&pb::Executor::Seek(Seek::default().save())).is_err());
    }

    /// A look keeps the [`FOUND`] places nearest to where the cleaner stood,
    /// and it starts on the nearest of them.
    #[test]
    fn a_look_keeps_the_nearest_places_nearest_first() {
        let mut world = World::new(Map::new(Size::new(40, 6), FLOOR));
        for x in 0..40 {
            stain(&mut world, Point::new(x, 1), 0.3);
        }
        let mut human = cleaner(CLEANER_SHIFTS[0]);
        human.body_mut().set_position((20.5, 1.5));
        world.step_in_time(&mut human);
        assert!(world.log_contains("looked over the district: 40 dirty places"), "{:?}", world.lines());
        let fields = human.brain().debug_fields();
        let field = |name: &str| fields.iter().find(|(field, _)| *field == name).map(|(_, value)| value.clone());
        assert_eq!(field("cleaning").as_deref(), Some("20, 1"), "the one it stands on first");
        let found = field("found").expect("the look is shown");
        assert_eq!(found.split("; ").count(), FOUND - 1, "{found}");
        assert!(
            found.split("; ").all(|cell| (12..=28).contains(&cell.split(", ").next().unwrap().parse::<i32>().unwrap())),
            "{found}"
        );
    }
}
