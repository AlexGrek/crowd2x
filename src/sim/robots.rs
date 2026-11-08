//! Robots: the district's machines — their body ([`Battery`]: charge and
//! condition) and the world's book of them ([`Robots`]).
//!
//! **A robot's one need is charge.** It drains as the hours go by
//! ([`HOURS_ON_A_CHARGE`]) and fills at a powered charging dock
//! ([`CHARGE_PER_HOUR`]); a robot heads for a dock with charge to spare
//! ([`TOP_UP`] off shift, [`MUST_CHARGE`] whatever it is doing — enough to
//! cross a district), and one that runs flat stops where it stands until an
//! electrician gives it [`RECHARGE`].
//!
//! **And it wears out** ([`DAYS_TO_WEAR_OUT`]). Under [`SAFE_CONDITION`] it
//! may break down, the likelier the more worn ([`Battery::break_chance`]); at
//! nothing it breaks for sure. A breakdown is a stop — an electrician's to
//! mend, back to [`SAFE_CONDITION`] — or, one time in [`EXPLODES`], an
//! explosion that hurts everybody near ([`blast`]) and leaves nothing of the
//! robot or what it carried. At [`SERVICE_AT`] it goes to the robot factory's
//! bay for an engineer's overhaul, back to whole.
//!
//! What the world keeps, [`Robots`]: who is stopped (for the electricians),
//! the fixes waiting to be felt (a fix is to the robot's own body, so it
//! feels it in its own react, as a bite or a dog's dinner is felt), who waits
//! at a bay (for the engineers), the factory's build ([`Works`]), the robots
//! just built and the explosions just set off (for the screen), and how many
//! people and robots there are (the factory builds up to a fifth of
//! everybody, [`Population::room_for_one`]).

use rand::RngExt;

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::clock::{DAY, HOUR};
use super::uid::{EntityType, Uid};

/// Cells a watched second: a little slower than an average person
/// ([`HUMAN_SPEED`](super::kinds::HUMAN_SPEED)), and every robot the same —
/// no pace of its own, no hurry, no panic.
pub const ROBOT_SPEED: f32 = super::kinds::HUMAN_SPEED * 0.8;

/// World hours a full charge lasts: twenty. The [`MUST_CHARGE`] a robot keeps
/// in hand is three of them — about 290 cells at [`ROBOT_SPEED`], more than
/// the walk to the furthest dock in a district.
pub const HOURS_ON_A_CHARGE: f32 = 20.0;

/// Charge a dock puts in an hour, in points of 100.
pub const CHARGE_PER_HOUR: f32 = 60.0;

/// Charge at or under which a robot with nothing on goes to top up.
pub const TOP_UP: f32 = 40.0;

/// Charge at or under which a robot goes to charge **whatever it is doing**:
/// enough left to reach a dock.
pub const MUST_CHARGE: f32 = 15.0;

/// Charge at which a robot is done charging.
pub const FULL: f32 = 95.0;

/// Charge an electrician gives a robot that ran flat.
pub const RECHARGE: f32 = 30.0;

/// World days from whole to worn out.
pub const DAYS_TO_WEAR_OUT: f32 = 8.0;

/// Condition under which a robot may break down — and what an electrician's
/// mend brings a broken one back to.
pub const SAFE_CONDITION: f32 = 60.0;

/// Condition at or under which a robot goes to the robot factory for an
/// overhaul.
pub const SERVICE_AT: f32 = 10.0;

/// The chance an hour of a breakdown at [`SERVICE_AT`]: rising in a line from
/// nothing at [`SAFE_CONDITION`].
pub const WORST_HOURLY_BREAK: f64 = 0.01;

/// The share of breakdowns that are explosions.
pub const EXPLODES: f64 = 0.3;

/// Health an explosion takes from somebody within [`BLAST_CORE`] of it...
pub const BLAST_DAMAGE: f32 = 100.0;
/// ...cells from it, all of it...
pub const BLAST_CORE: f32 = 1.0;
/// ...falling to nothing this far out.
pub const BLAST_RADIUS: f32 = 4.0;

/// Steel a robot is made of, taken off the factory's parts rack when its
/// build starts.
pub const ROBOT_STEEL: u32 = 20;

/// World hours of engineers' work at the bay a robot takes.
pub const ASSEMBLY_HOURS: f32 = 8.0;

/// World seconds from one robot built to the next falling due: two days.
pub const BUILD_EVERY: f64 = 2.0 * DAY as f64;

/// Robots a district starts with.
pub const ROBOTS_AT_START: usize = 2;

/// Robots may be one in this many of everybody: a fifth.
pub const ONE_IN: u32 = 5;

/// What a person killed by an exploding robot died of.
pub const ROBOT_EXPLOSION: &str = "a robot explosion";

/// How many explosions the screen is told of at most, the latest.
const EXPLOSIONS_KEPT: usize = 16;

/// Whether it works.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Running,
    /// Ran out of charge: stopped until an electrician recharges it.
    Flat,
    /// Broke down: stopped until an electrician mends it.
    Broken,
}

impl State {
    pub const fn name(self) -> &'static str {
        match self {
            State::Running => "running",
            State::Flat => "flat",
            State::Broken => "broken",
        }
    }
}

/// What befell a robot this tick, if anything ([`Battery::advance`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mishap {
    /// Ran flat and stopped.
    Flat,
    /// Broke down and stopped.
    Broken,
    /// Blew up.
    Explode,
}

/// A robot's body: its charge and its condition, 0 to 100, and whether it
/// runs. `Copy`, inline in the robot.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Battery {
    charge: f32,
    condition: f32,
    state: State,
}

impl Battery {
    /// Charged full and whole, as a robot comes out of the factory.
    pub const fn fresh() -> Battery {
        Battery { charge: 100.0, condition: 100.0, state: State::Running }
    }

    pub fn charge(&self) -> f32 {
        self.charge
    }

    pub fn condition(&self) -> f32 {
        self.condition
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_running(&self) -> bool {
        self.state == State::Running
    }

    #[cfg(test)]
    pub(crate) fn with(charge: f32, condition: f32) -> Battery {
        Battery { charge, condition, state: State::Running }
    }

    /// The chance an hour of breaking down at `condition`: nothing at
    /// [`SAFE_CONDITION`] or over, [`WORST_HOURLY_BREAK`] at [`SERVICE_AT`],
    /// in a line between and on past it — and certain at nothing.
    pub fn break_chance(condition: f32) -> f64 {
        if condition <= 0.0 {
            1.0
        } else if condition >= SAFE_CONDITION {
            0.0
        } else {
            WORST_HOURLY_BREAK * (SAFE_CONDITION - condition) as f64 / (SAFE_CONDITION - SERVICE_AT) as f64
        }
    }

    /// `world_dt` world seconds go by: charged at a dock or drained, and worn.
    /// Then the dice, from `rng`: a breakdown, likelier the more worn, and
    /// one in [`EXPLODES`] of those an explosion. A robot that is not running
    /// does nothing at all.
    pub fn advance(&mut self, world_dt: f32, charging: bool, rng: &mut impl RngExt) -> Option<Mishap> {
        if !self.is_running() {
            return None;
        }
        if charging {
            self.charge = (self.charge + CHARGE_PER_HOUR * world_dt / HOUR).min(100.0);
        } else {
            self.charge = (self.charge - 100.0 * world_dt / (HOURS_ON_A_CHARGE * HOUR)).max(0.0);
        }
        self.condition = (self.condition - 100.0 * world_dt / (DAYS_TO_WEAR_OUT * DAY)).max(0.0);
        if self.charge <= 0.0 {
            self.state = State::Flat;
            return Some(Mishap::Flat);
        }
        if self.condition < SAFE_CONDITION {
            let chance = Battery::break_chance(self.condition) * (world_dt / HOUR) as f64;
            if self.condition <= 0.0 || rng.random::<f64>() < chance {
                if rng.random::<f64>() < EXPLODES {
                    return Some(Mishap::Explode);
                }
                self.state = State::Broken;
                return Some(Mishap::Broken);
            }
        }
        None
    }

    /// A fix, felt: charge put in, a mend, an overhaul.
    pub fn feel(&mut self, fix: FixKind) {
        match fix {
            FixKind::Recharge => {
                self.charge = (self.charge + RECHARGE).min(100.0);
                if self.state == State::Flat {
                    self.state = State::Running;
                }
            }
            FixKind::Mend => {
                self.condition = self.condition.max(SAFE_CONDITION);
                if self.state == State::Broken {
                    self.state = State::Running;
                }
            }
            FixKind::Overhaul => self.condition = 100.0,
        }
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Battery {
        let Battery { charge, condition, state } = *self;
        pb::Battery {
            charge,
            condition,
            state: match state {
                State::Running => pb::RobotState::Running,
                State::Flat => pb::RobotState::Flat,
                State::Broken => pb::RobotState::Broken,
            } as i32,
        }
    }

    pub(crate) fn restore(saved: &pb::Battery) -> Result<Battery, Invalid> {
        if !(0.0..=100.0).contains(&saved.charge) || !(0.0..=100.0).contains(&saved.condition) {
            return invalid(format!("a battery at {} charge and {} condition", saved.charge, saved.condition));
        }
        let state = match pb::known::<pb::RobotState>(saved.state, "robot state")? {
            pb::RobotState::Running => State::Running,
            pb::RobotState::Flat => State::Flat,
            pb::RobotState::Broken => State::Broken,
        };
        Ok(Battery { charge: saved.charge, condition: saved.condition, state })
    }
}

/// What a fix does ([`Battery::feel`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FixKind {
    /// An electrician's charge into a robot that ran flat: [`RECHARGE`].
    Recharge,
    /// An electrician's mend of a broken robot: back to [`SAFE_CONDITION`].
    Mend,
    /// An engineer's overhaul at the factory: whole again.
    Overhaul,
}

/// A fix done to `robot`, for it to feel next tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fix {
    pub robot: Uid,
    pub kind: FixKind,
}

/// A robot standing stopped, and why: an electrician's to see to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stopped {
    pub uid: Uid,
    pub cell: Point,
    pub why: State,
}

/// The robot factory's build: when the next robot falls due, and how far the
/// one being built has come.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Works {
    /// World seconds ([`Clock::elapsed`](super::clock::Clock::elapsed)) from
    /// which the next robot may be started.
    next_due: f64,
    /// World seconds of work done on the robot being built.
    progress: f32,
    /// Its steel is in: a build under way, finished whatever else happens.
    started: bool,
}

impl Default for Works {
    /// The first robot falls due [`BUILD_EVERY`] after the world begins: it
    /// starts with [`ROBOTS_AT_START`].
    fn default() -> Works {
        Works { next_due: BUILD_EVERY, progress: 0.0, started: false }
    }
}

impl Works {
    /// Whether the engineers have a robot to build: one under way, or one
    /// due with room for it.
    pub fn due(&self, now: f64, population: Population) -> bool {
        self.started || (now >= self.next_due && population.room_for_one())
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn next_due(&self) -> f64 {
        self.next_due
    }

    /// The steel is in: the build is under way.
    pub fn start(&mut self) {
        self.started = true;
        self.progress = 0.0;
    }

    /// `seconds` of work on it; `true` once it is built, and the next falls
    /// due [`BUILD_EVERY`] after `now`.
    pub fn work(&mut self, seconds: f32, now: f64) -> bool {
        self.progress += seconds;
        if self.progress < ASSEMBLY_HOURS * HOUR {
            return false;
        }
        *self = Works { next_due: now + BUILD_EVERY, progress: 0.0, started: false };
        true
    }

    pub(crate) fn save(&self) -> pb::RobotWorks {
        let Works { next_due, progress, started } = *self;
        pb::RobotWorks { next_due, progress, started }
    }

    pub(crate) fn restore(saved: &pb::RobotWorks) -> Result<Works, Invalid> {
        if !saved.next_due.is_finite() || !saved.progress.is_finite() || saved.progress < 0.0 {
            return invalid(format!("robot works due at {} with {} done", saved.next_due, saved.progress));
        }
        Ok(Works { next_due: saved.next_due, progress: saved.progress, started: saved.started })
    }
}

/// How many people and robots there are: what the factory builds up to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Population {
    pub humans: u32,
    pub robots: u32,
}

impl Population {
    /// Whether one more robot keeps robots to [`ONE_IN`] of everybody.
    pub fn room_for_one(&self) -> bool {
        (self.robots + 1) * ONE_IN <= self.humans + self.robots + 1
    }
}

/// An explosion, for the screen: where, and a number to tell it from the
/// ones already drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Explosion {
    pub at: Point,
    pub serial: u64,
}

/// The health an explosion `distance` cells away takes: [`BLAST_DAMAGE`]
/// within [`BLAST_CORE`], nothing at [`BLAST_RADIUS`], in a line between.
pub fn blast(distance: f32) -> u8 {
    if distance <= BLAST_CORE {
        return BLAST_DAMAGE as u8;
    }
    let left = ((BLAST_RADIUS - distance) / (BLAST_RADIUS - BLAST_CORE)).clamp(0.0, 1.0);
    (BLAST_DAMAGE * left).round() as u8
}

/// The world's book of robots. See the module docs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Robots {
    /// Sorted by uid.
    stopped: Vec<Stopped>,
    /// Filed in one world step, felt in the next tick's react, forgotten at
    /// the next world step: saved.
    fixes: Vec<Fix>,
    /// Who waited at a bay last tick, sorted by uid: what the engineers see.
    waiting: Vec<(Uid, Point)>,
    /// Who is saying so this tick: `waiting` at the next world step.
    asking: Vec<(Uid, Point)>,
    works: Works,
    /// Bays a robot was finished at this tick: spawned at the end of it.
    assembled: Vec<Point>,
    explosions: Vec<Explosion>,
    serial: u64,
    population: Population,
}

impl Robots {
    /// What the world step does first: last tick's fixes have been felt, and
    /// who said they wait at a bay is who waits there now.
    pub fn begin_tick(&mut self) {
        self.fixes.clear();
        std::mem::swap(&mut self.waiting, &mut self.asking);
        self.asking.clear();
        self.waiting.sort_unstable_by_key(|&(uid, _)| uid);
        self.waiting.dedup_by_key(|&mut (uid, _)| uid);
    }

    /// `uid` stopped at `cell`, `why`.
    pub fn stop(&mut self, uid: Uid, cell: Point, why: State) {
        match self.stopped.binary_search_by_key(&uid, |stopped| stopped.uid) {
            Ok(i) => self.stopped[i] = Stopped { uid, cell, why },
            Err(i) => self.stopped.insert(i, Stopped { uid, cell, why }),
        }
    }

    /// `uid` runs again, or is gone.
    pub fn unstop(&mut self, uid: Uid) -> Option<Stopped> {
        let i = self.stopped.binary_search_by_key(&uid, |stopped| stopped.uid).ok()?;
        Some(self.stopped.remove(i))
    }

    pub fn stopped(&self) -> &[Stopped] {
        &self.stopped
    }

    /// The robot stopped in `cell`, if one is.
    pub fn stopped_at(&self, cell: Point) -> Option<Stopped> {
        self.stopped.iter().copied().find(|stopped| stopped.cell == cell)
    }

    pub fn fix(&mut self, fix: Fix) {
        self.fixes.push(fix);
    }

    /// The fixes done to `robot` last tick.
    pub fn fixes_to(&self, robot: Uid) -> impl Iterator<Item = FixKind> + '_ {
        self.fixes.iter().filter(move |fix| fix.robot == robot).map(|fix| fix.kind)
    }

    /// `uid` waits at the bay, standing in `cell`.
    pub fn await_service(&mut self, uid: Uid, cell: Point) {
        self.asking.push((uid, cell));
    }

    /// Who waited at a bay as the last world step ended, and where it stood.
    pub fn waiting(&self) -> &[(Uid, Point)] {
        &self.waiting
    }

    /// The robot waiting in `cell`, if one is — and it is no longer waiting
    /// for anybody else: an engineer has it.
    pub fn take_waiting(&mut self, cell: Point) -> Option<Uid> {
        let i = self.waiting.iter().position(|&(_, at)| at == cell)?;
        Some(self.waiting.remove(i).0)
    }

    pub fn works(&self) -> &Works {
        &self.works
    }

    pub fn works_mut(&mut self) -> &mut Works {
        &mut self.works
    }

    /// A robot was finished at the bay in `at`.
    pub fn assemble(&mut self, at: Point) {
        self.assembled.push(at);
    }

    /// The bays robots were finished at this tick, emptied.
    pub fn take_assembled(&mut self) -> Vec<Point> {
        std::mem::take(&mut self.assembled)
    }

    /// A robot blew up at `at`.
    pub fn explode(&mut self, at: Point) {
        self.serial += 1;
        self.explosions.push(Explosion { at, serial: self.serial });
        if self.explosions.len() > EXPLOSIONS_KEPT {
            self.explosions.remove(0);
        }
    }

    /// The latest explosions, oldest first.
    pub fn explosions(&self) -> &[Explosion] {
        &self.explosions
    }

    pub fn population(&self) -> Population {
        self.population
    }

    /// Count the living again: people and robots.
    pub fn recount<'a>(&mut self, entities: impl Iterator<Item = &'a dyn super::GameEntity>) {
        let mut population = Population::default();
        for entity in entities {
            if entity.is_dead() {
                continue;
            }
            match entity.kind() {
                Some(EntityType::Human) => population.humans += 1,
                Some(EntityType::Robot) => population.robots += 1,
                _ => {}
            }
        }
        self.population = population;
    }

    /// Who was stopped, from the robots themselves: on load.
    pub fn restore_stopped<'a>(&mut self, entities: impl Iterator<Item = &'a dyn super::GameEntity>) {
        self.stopped.clear();
        for entity in entities {
            if let Some(why) = entity.stopped() {
                self.stop(entity.uid(), entity.center_position(), why);
            }
        }
    }

    // --- saving ---

    pub(crate) fn save_works(&self) -> pb::RobotWorks {
        self.works.save()
    }

    pub(crate) fn save_fixes(&self) -> Vec<pb::RobotFix> {
        self.fixes
            .iter()
            .map(|&Fix { robot, kind }| pb::RobotFix {
                robot: robot.raw(),
                kind: match kind {
                    FixKind::Recharge => pb::FixKind::Recharge,
                    FixKind::Mend => pb::FixKind::Mend,
                    FixKind::Overhaul => pb::FixKind::Overhaul,
                } as i32,
            })
            .collect()
    }

    /// The works and the fixes a save kept; absent in a save from before
    /// robots, a factory with its first robot due in two days and nothing
    /// to feel.
    pub(crate) fn restore(&mut self, works: Option<&pb::RobotWorks>, fixes: &[pb::RobotFix]) -> Result<(), Invalid> {
        self.works = works.map(Works::restore).transpose()?.unwrap_or_default();
        self.fixes.clear();
        for fix in fixes {
            let Some(robot) = Uid::from_raw(fix.robot).filter(|uid| uid.kind() == Some(EntityType::Robot)) else {
                return invalid(format!("a fix for {:#x}, which is no robot", fix.robot));
            };
            let kind = match pb::known::<pb::FixKind>(fix.kind, "fix")? {
                pb::FixKind::Recharge => FixKind::Recharge,
                pb::FixKind::Mend => FixKind::Mend,
                pb::FixKind::Overhaul => FixKind::Overhaul,
            };
            self.fixes.push(Fix { robot, kind });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    fn hours(battery: &mut Battery, hours: f32, charging: bool) -> Option<Mishap> {
        let mut rng = SmallRng::seed_from_u64(1);
        let step = 60.0;
        let mut mishap = None;
        for _ in 0..(hours * HOUR / step) as u32 {
            mishap = mishap.or(battery.advance(step, charging, &mut rng));
        }
        mishap
    }

    #[test]
    fn a_full_charge_lasts_twenty_hours_and_a_dock_fills_it_in_under_two() {
        let mut battery = Battery::fresh();
        assert_eq!(hours(&mut battery, 10.0, false), None);
        assert!((battery.charge() - 50.0).abs() < 0.1, "half gone in ten hours: {}", battery.charge());
        assert_eq!(hours(&mut battery, 0.5, true), None);
        assert!((battery.charge() - 80.0).abs() < 0.1, "thirty points in half an hour: {}", battery.charge());
        let mut flat = Battery::with(1.0, 100.0);
        assert_eq!(hours(&mut flat, 1.0, false), Some(Mishap::Flat));
        assert_eq!(flat.state(), State::Flat);
        let before = flat;
        assert_eq!(hours(&mut flat, 5.0, false), None, "a stopped robot does nothing");
        assert_eq!(flat, before);
    }

    #[test]
    fn a_robot_wears_out_in_eight_days() {
        let mut battery = Battery::fresh();
        let mut rng = SmallRng::seed_from_u64(1);
        let _ = battery.advance(4.0 * DAY, true, &mut rng);
        assert!((battery.condition() - 50.0).abs() < 0.01, "{}", battery.condition());
    }

    #[test]
    fn the_chance_of_breaking_down_is_nothing_when_sound_rises_with_wear_and_is_certain_at_nothing() {
        assert_eq!(Battery::break_chance(100.0), 0.0);
        assert_eq!(Battery::break_chance(SAFE_CONDITION), 0.0);
        let mut last = 0.0;
        for condition in [59.0, 50.0, 30.0, 10.0, 1.0] {
            let chance = Battery::break_chance(condition);
            assert!(chance > last, "{condition}: {chance}");
            last = chance;
        }
        assert!((Battery::break_chance(SERVICE_AT) - WORST_HOURLY_BREAK).abs() < 1e-9);
        assert_eq!(Battery::break_chance(0.0), 1.0);
        let mut done = Battery::with(100.0, 0.0);
        let mishap = done.advance(1.0, false, &mut SmallRng::seed_from_u64(3));
        assert!(matches!(mishap, Some(Mishap::Broken | Mishap::Explode)), "{mishap:?}");
    }

    #[test]
    fn three_breakdowns_in_ten_are_explosions() {
        let (mut stops, mut explosions) = (0, 0);
        for seed in 0..4000 {
            let mut battery = Battery::with(100.0, 0.0);
            match battery.advance(1.0, false, &mut SmallRng::seed_from_u64(seed)) {
                Some(Mishap::Broken) => stops += 1,
                Some(Mishap::Explode) => explosions += 1,
                other => panic!("{other:?}"),
            }
        }
        let share = explosions as f64 / (stops + explosions) as f64;
        assert!((share - EXPLODES).abs() < 0.03, "{share}");
    }

    #[test]
    fn a_blast_takes_everything_close_by_and_nothing_four_cells_out() {
        assert_eq!(blast(0.0), 100);
        assert_eq!(blast(1.0), 100);
        assert_eq!(blast(2.5), 50);
        assert_eq!(blast(4.0), 0);
        assert_eq!(blast(6.0), 0);
    }

    #[test]
    fn fixes_bring_a_robot_back() {
        let mut flat = Battery { charge: 0.0, condition: 40.0, state: State::Flat };
        flat.feel(FixKind::Recharge);
        assert_eq!((flat.charge(), flat.state()), (RECHARGE, State::Running));
        let mut broken = Battery { charge: 50.0, condition: 20.0, state: State::Broken };
        broken.feel(FixKind::Mend);
        assert_eq!((broken.condition(), broken.state()), (SAFE_CONDITION, State::Running));
        broken.feel(FixKind::Overhaul);
        assert_eq!(broken.condition(), 100.0);
    }

    #[test]
    fn robots_are_built_up_to_a_fifth_of_everybody() {
        assert!(Population { humans: 8, robots: 1 }.room_for_one(), "2 of 10");
        assert!(!Population { humans: 8, robots: 2 }.room_for_one(), "3 of 11 is too many");
        assert!(!Population { humans: 3, robots: 0 }.room_for_one());
        assert!(Population { humans: 4, robots: 0 }.room_for_one());
        let mut works = Works::default();
        assert!(!works.due(0.0, Population { humans: 100, robots: 2 }), "the first is due in two days");
        assert!(works.due(BUILD_EVERY, Population { humans: 100, robots: 2 }));
        assert!(!works.due(BUILD_EVERY, Population { humans: 10, robots: 2 }), "no room");
        works.start();
        assert!(works.due(0.0, Population { humans: 10, robots: 2 }), "under way, it is finished");
        assert!(!works.work(ASSEMBLY_HOURS * HOUR - 1.0, 100.0));
        assert!(works.work(1.0, 100.0));
        assert!(!works.is_started());
        assert_eq!(works.next_due(), 100.0 + BUILD_EVERY);
    }

    #[test]
    fn a_battery_and_the_works_survive_a_save() {
        let battery = Battery { charge: 12.5, condition: 33.0, state: State::Broken };
        assert_eq!(Battery::restore(&battery.save()).unwrap(), battery);
        let mut robots = Robots::default();
        robots.works_mut().start();
        let _ = robots.works_mut().work(HOUR, 0.0);
        let robot = Uid::new(EntityType::Robot, 9);
        robots.fix(Fix { robot, kind: FixKind::Mend });
        let mut loaded = Robots::default();
        loaded.restore(Some(&robots.save_works()), &robots.save_fixes()).unwrap();
        assert_eq!(loaded.works(), robots.works());
        assert_eq!(loaded.fixes_to(robot).collect::<Vec<_>>(), vec![FixKind::Mend]);
        let human = Uid::new(EntityType::Human, 9);
        assert!(loaded.restore(None, &[pb::RobotFix { robot: human.raw(), kind: 0 }]).is_err());
    }

    #[test]
    fn who_waits_at_a_bay_is_who_said_so_last_tick() {
        let mut robots = Robots::default();
        let robot = Uid::new(EntityType::Robot, 4);
        robots.await_service(robot, Point::new(3, 3));
        assert!(robots.waiting().is_empty());
        robots.begin_tick();
        assert_eq!(robots.waiting(), &[(robot, Point::new(3, 3))]);
        assert_eq!(robots.take_waiting(Point::new(3, 3)), Some(robot));
        robots.begin_tick();
        assert!(robots.waiting().is_empty(), "it stopped saying so");
    }
}

/// A robot through the brain and body it has: a `World` of one.
#[cfg(test)]
mod behaviour {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::brain::goal::GoalId;
    use crate::sim::kinds::{Facing, Robot};
    use crate::sim::testing::{prop_at, World};
    use crate::sim::walker::ROUTES_ASKED;
    use crate::sim::GameEntity;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    const DOCK: Point = Point::new(10, 5);

    fn robot(battery: Battery, at: Point) -> Robot {
        let mut rng = SmallRng::seed_from_u64(5);
        let mut robot = Robot::new(crate::sim::Uid::new(EntityType::Robot, 9), at, Facing::Right, &mut rng);
        robot.set_battery(battery);
        robot
    }

    fn with_dock() -> World {
        let mut map = Map::new(Size::new(16, 12), FLOOR);
        prop_at(&mut map, "charging dock", DOCK);
        World::new(map)
    }

    fn run(world: &mut World, robot: &mut Robot, ticks: u32) {
        for _ in 0..ticks {
            world.step_in_time(robot);
        }
    }

    /// Low on charge and with nothing to do, a robot walks to the dock,
    /// stays plugged in until it is full and goes about its business.
    #[test]
    fn an_idle_robot_under_forty_percent_charges_to_full() {
        let mut world = with_dock();
        let mut robot = robot(Battery::with(35.0, 100.0), Point::new(2, 2));
        let mut peak = 0.0f32;
        let mut after = None;
        for _ in 0..40 {
            run(&mut world, &mut robot, 500);
            peak = peak.max(robot.battery().charge());
            if peak >= FULL - 2.0 && after.is_none() {
                after = Some(robot.current_goal());
            }
        }
        assert!(peak >= FULL - 2.0, "charged to {peak}");
        assert!(robot.current_goal() != Some(GoalId::Charge) || robot.battery().charge() < FULL, "it left when full");
        assert!(robot.battery().is_running());
    }

    /// Above the top-up line it has no reason to.
    #[test]
    fn a_robot_with_charge_to_spare_leaves_the_dock_alone() {
        let mut world = with_dock();
        let mut robot = robot(Battery::with(80.0, 100.0), Point::new(2, 2));
        run(&mut world, &mut robot, 2_000);
        assert!(robot.brain().goals().priority(GoalId::Charge) == 0.0);
        assert!(robot.battery().charge() < 80.0, "drained, not charged");
    }

    /// Run flat, it stops where it is and is on the book for an electrician;
    /// a recharge fix puts 30 in it and it goes on.
    #[test]
    fn a_flat_robot_stops_waits_and_is_recharged_by_a_fix() {
        let mut world = World::new(Map::new(Size::new(16, 12), FLOOR));
        let mut robot = robot(Battery::with(0.01, 100.0), Point::new(4, 4));
        run(&mut world, &mut robot, 400);
        assert_eq!(robot.battery().state(), State::Flat);
        assert_eq!(world.facilities.robots.stopped().len(), 1);
        let cell = robot.center_position();
        run(&mut world, &mut robot, 400);
        assert_eq!(robot.center_position(), cell, "stopped means stopped");
        world.facilities.robots.fix(Fix { robot: robot.uid(), kind: FixKind::Recharge });
        run(&mut world, &mut robot, 2);
        assert!(robot.battery().is_running());
        assert!(robot.battery().charge() >= RECHARGE - 1.0, "{}", robot.battery().charge());
    }

    /// With no charging dock anywhere the want is held off, not retried every
    /// tick.
    #[test]
    fn a_robot_with_nowhere_to_charge_does_not_ask_for_routes_every_tick() {
        let mut world = World::new(Map::new(Size::new(16, 12), FLOOR));
        let mut robot = robot(Battery::with(30.0, 100.0), Point::new(4, 4));
        ROUTES_ASKED.with(|asked| asked.set(0));
        run(&mut world, &mut robot, 3_000);
        let asked = ROUTES_ASKED.with(|asked| asked.get());
        assert!(asked < 300, "{asked} routes in 3000 ticks");
    }

    /// Robots walk at the one pace, whatever they are doing.
    #[test]
    fn a_robot_walks_at_robot_speed() {
        let mut world = with_dock();
        let mut robot = robot(Battery::with(30.0, 100.0), Point::new(1, 1));
        let mut last = robot.position();
        let mut fastest = 0.0f32;
        for _ in 0..600 {
            world.step_in_time(&mut robot);
            let now = robot.position();
            fastest = fastest.max(((now.0 - last.0).powi(2) + (now.1 - last.1).powi(2)).sqrt() / world.dt);
            last = now;
        }
        assert!(fastest <= ROBOT_SPEED * 1.01, "{fastest} cells/s");
    }
}
