//! Jobs: what somebody does for a living, where, and when — [`Job`] — and
//! [`Workplaces`], which desks, bins and guard posts have been given to whom,
//! for which shift.
//!
//! **A job is handed out at spawn, the way a bed is** (`GameState::give_a_job`):
//! an arrival is given the nearest workplace nobody else has and that it may
//! use, and is unemployed when there is none left. The job then lives in the
//! unit's own brain ([`Brain::job`](super::brain::Brain::job)) — a unit knows
//! where it works and when, the way it knows which bed is its own — and this
//! module's [`Workplaces`] is only the bookkeeping that stops two arrivals
//! being given one desk. Nothing in a tick reads it.
//!
//! What a job *does* to a unit is the brain's: `WorkRoutine` wants the job's
//! goal ([`JobKind::goal`]) while the shift is due, and that goal's executor
//! does the work. What working does to the body is `Energy`'s
//! ([`Event::Worked`](super::biology::Event)).
//!
//! Three jobs, offered in this order ([`JobKind::ALL`]):
//!
//! * the **office worker**: a computer, worked an hour at a time
//!   (`WorkGoal`), nine to five, Monday to Friday;
//! * the **scrap collector**: a scrap bin in the warehouse, and the heaps of
//!   scrap out in the wasteland — out to a heap, salvage a packful, back to
//!   the bin and store it (`CollectGoal`), every trip got ready for first,
//!   every need seen to before it goes out (`ReadyRoutine`,
//!   [`JobDef::goes_out_for`]). One collector to a bin, office hours;
//! * the **guard**: a guard post by one of the gates in the district's wall,
//!   stood at an hour at a time (`WorkGoal` again), round the clock: three
//!   eight-hour [`WATCHES`] every day of the week, a guard to each, so a
//!   post is three workplaces, one per watch;
//! * the **undertaker**: a crematorium, and the dead wherever they fell —
//!   to the nearest body, take it up, carry it back and cremate it
//!   (`BuryGoal`). Round the clock like a guard, the three [`WATCHES`] of a
//!   crematorium an undertaker each;
//! * the **steelworker**: a furnace in the steel factory — scrap metal out of
//!   the factory's hoppers, smelted into steel at the furnace, the steel
//!   onto the factory's pallets (`SmeltGoal`). Office hours;
//! * the **courier**: a scrap hopper in the steel factory — scrap metal from
//!   the warehouse's bins to the hopper, and steel from the factory's
//!   pallets to the warehouse's rack (`HaulGoal`). Office hours;
//! * the **farmer**: a grain bin at the farm, and a field of wheat out in
//!   the wasteland's good soil — sown, tended every day, harvested ripe and
//!   the wheat stored in the bin (`FarmGoal`, `sim::fields`). Nine to five,
//!   [`FARM_HOURS`]: every day but Sunday;
//! * the **power plant worker**: the control panel of the power plant, which
//!   powers the district — stood by an hour at a time (`WorkGoal`, like a
//!   desk or a post) and nothing else yet. Round the clock like a guard: the
//!   three [`WATCHES`] of the panel a worker each;
//! * the **king**: the throne in the district's palace, the one there is —
//!   court held an hour at a time (`WorkGoal`) and nothing else yet: the
//!   decisions a king makes are to come. [`COURT_HOURS`], every day but
//!   Sunday. A king carries itself as one ([`Bearing`], [`ROYAL`]): its
//!   robes and crown always on, on duty or not, and a stately walk.
//!
//! **A worker sleeps by its shift** ([`Shift::is_bedtime`]): the eight hours
//! that end [`UP_BEFORE_SHIFT`] before it starts. For office hours that is
//! the clock's own night, ten to six; the night watch sleeps the afternoon.

use std::collections::BTreeMap;

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::brain::GoalId;
use super::catalogue::catalogue;
use super::clothing::Clothing;
use super::clock::{Clock, Weekday, BEDTIME, GETTING_UP, HOUR};
use super::feature::FeatureKind;
use super::inventory::Inventory;
use super::item::ItemKind;
use super::uid::Uid;

/// How long before a shift starts a worker sets off for it, in **world**
/// seconds: half an hour, so somebody across the district is at the desk by
/// the hour rather than leaving home at it.
pub const COMMUTE: f32 = 0.5 * HOUR;

/// How long before a watch a guard sets off: two hours. A post is in the
/// wall at the edge of the district and its guards live all over it — the
/// last few hired get the posts nobody nearer took, up to a couple of hours'
/// walk away — and a watch is only kept if the next guard is there when the
/// last one leaves. One who lives nearby is early, and waits by the post.
pub const GUARD_COMMUTE: f32 = 2.0 * HOUR;

/// How long before court a king sets off for the palace: an hour and a half.
/// The palace is in the middle of the district, and a king walks it at
/// [`KING_GAIT`] — from a house on the edge, an hour and a half's walk.
pub const KING_COMMUTE: f32 = 1.5 * HOUR;

/// How fast a king walks, as a share of its own pace: slowly. A king does not
/// run, either — late, or starving, it walks briskly at best (the hurry is a
/// share of its pace too).
pub const KING_GAIT: f32 = 0.6;

/// How somebody in a job carries themselves, on duty or off: what the job
/// does to a person beyond the hours it keeps them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bearing {
    /// How fast it walks, as a share of its own pace — and of any hurry it is
    /// in (`Walker::set_gait`, set from the job at hire, let go and load).
    pub gait: f32,
    /// Whether its uniform is worn **always**, or only while the shift is
    /// due (`GameEntity::worn`).
    pub always_in_uniform: bool,
    /// What it wears on its head with the uniform, over its hair:
    /// `assets/human/hat_<hat>.png` (`characters::human::hat_path`), drawn by
    /// `tools/human_art.py`.
    pub hat: Option<&'static str>,
}

/// Every job but the king's: its own pace, its uniform for the shift, its
/// own hair.
pub const WORKER: Bearing = Bearing { gait: 1.0, always_in_uniform: false, hat: None };

/// The king's: a stately walk ([`KING_GAIT`]), its robes always on, and a
/// crown.
pub const ROYAL: Bearing = Bearing { gait: KING_GAIT, always_in_uniform: true, hat: Some("crown") };

/// Everything about one kind of job: its row in [`JobKind`]'s catalogue.
pub struct JobDef {
    pub name: &'static str,
    /// "an office worker", "a scrap collector": for a log line.
    pub article: &'static str,
    /// The feature the job is done at, and so what a workplace is handed out
    /// from. The prop it is ([`FeatureKind::prop`]) is what the workplace is
    /// called on the job menu and in the log.
    pub workplace: FeatureKind,
    /// The goal the job is done by: what `WorkRoutine` raises while the
    /// shift is due. It needs an executor ([`GoalDef`](super::brain::goal::GoalDef)).
    pub goal: GoalId,
    /// How long before its shift somebody sets off, in **world** seconds.
    pub commute: f32,
    /// The shifts a workplace of the kind is worked in, **one worker to
    /// each**, in the order they are handed out.
    pub shifts: &'static [Shift],
    /// The name of the uniform worn to it ([`Clothing::Uniform`]), which is
    /// also how its art is found: `assets/human/clothes_uniform_<job>.png`,
    /// drawn by `tools/human_art.py`.
    pub uniform: &'static str,
    /// The job menu's `now` line while the shift is due and the job's goal is
    /// in charge: what it is doing about it, given whether it is at its own
    /// workplace and what it carries.
    pub now: fn(at_work: bool, pack: Option<&Inventory>) -> &'static str,
    /// What a trip out of the district brings back, for a job worked on
    /// trips out into the wasteland — and so a job whose worker **gets
    /// ready** before each one (`ReadyRoutine`): with none of it in the pack
    /// and still in the district, it sees to every need first. `None` for a
    /// job that never takes anybody far from a fridge and a toilet.
    pub goes_out_for: Option<ItemKind>,
    /// How somebody in it carries themselves, on duty or off: [`WORKER`], or
    /// the king's [`ROYAL`].
    pub bearing: Bearing,
}

catalogue! {
    /// Every kind of job there is, **in the order a job is offered to an
    /// arrival** (`GameState::give_a_job`): the bank's desks first, then the
    /// bins, then the gates, then the crematoria, then the steel factory's
    /// furnaces and hoppers, then the farm's grain bins, then the power
    /// plant's control panel, then the palace's throne, then the electrician crew's box. A new job goes on the
    /// end, and so is hired last.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum JobKind: JobDef {
        /// Sits at a computer and works it. Tiring, and no fun.
        OfficeWorker = 0 => JobDef {
            name: "office worker",
            article: "an office worker",
            workplace: FeatureKind::Workstation,
            goal: GoalId::Work,
            commute: COMMUTE,
            shifts: &[OFFICE_HOURS],
            uniform: "uniform office worker",
            now: |at_work, _| if at_work { "working" } else { "on the way to work" },
            goes_out_for: None,
            bearing: WORKER,
        },
        /// Brings scrap metal in from the heaps in the wasteland and stores it
        /// in its own bin in the warehouse. As tiring as office work, and as
        /// little fun.
        ScrapCollector = 1 => JobDef {
            name: "scrap collector",
            article: "a scrap collector",
            workplace: FeatureKind::ScrapBin,
            goal: GoalId::Collect,
            commute: COMMUTE,
            shifts: &[OFFICE_HOURS],
            uniform: "uniform scrap collector",
            now: |at_work, _| if at_work { "storing scrap" } else { "collecting scrap" },
            goes_out_for: Some(ItemKind::Scrap),
            bearing: WORKER,
        },
        /// Stands at a guard post by a gate in the district's wall for one of
        /// the three [`WATCHES`] of the day. Worked like a desk, and as tiring;
        /// a gate is at the edge of the district, so it sets off
        /// [`GUARD_COMMUTE`] before.
        Guard = 2 => JobDef {
            name: "guard",
            article: "a guard",
            workplace: FeatureKind::GuardPost,
            goal: GoalId::Work,
            commute: GUARD_COMMUTE,
            shifts: &WATCHES,
            uniform: "uniform guard",
            now: |at_work, _| if at_work { "on watch" } else { "on the way to the gate" },
            goes_out_for: None,
            bearing: WORKER,
        },
        /// Takes up the dead and carries them to its own crematorium, on one of
        /// the three [`WATCHES`]: the dead do not wait for office hours.
        Undertaker = 3 => JobDef {
            name: "undertaker",
            article: "an undertaker",
            workplace: FeatureKind::Crematorium,
            goal: GoalId::Bury,
            commute: COMMUTE,
            shifts: &WATCHES,
            uniform: "uniform undertaker",
            now: |at_work, _| if at_work { "cremating" } else { "seeing to the dead" },
            goes_out_for: None,
            bearing: WORKER,
        },
        /// Smelts scrap metal into steel at its own furnace in the steel
        /// factory, fetching the scrap from the factory's hoppers and putting the
        /// steel on its pallets.
        Steelworker = 4 => JobDef {
            name: "steelworker",
            article: "a steelworker",
            workplace: FeatureKind::Furnace,
            goal: GoalId::Smelt,
            commute: COMMUTE,
            shifts: &[OFFICE_HOURS],
            uniform: "uniform steelworker",
            now: |at_work, _| if at_work { "smelting steel" } else { "fetching scrap, storing steel" },
            goes_out_for: None,
            bearing: WORKER,
        },
        /// Carries scrap metal from the warehouse to its own hopper in the steel
        /// factory, and steel from the factory back to the warehouse.
        Courier = 5 => JobDef {
            name: "courier",
            article: "a courier",
            workplace: FeatureKind::ScrapHopper,
            goal: GoalId::Haul,
            commute: COMMUTE,
            shifts: &[OFFICE_HOURS],
            uniform: "uniform courier",
            now: |_, pack| {
                let carries = |item| pack.is_some_and(|pack| pack.count(item) > 0);
                if carries(ItemKind::Steel) {
                    "carrying steel to the warehouse"
                } else if carries(ItemKind::Scrap) {
                    "carrying scrap to the factory"
                } else {
                    "fetching a load"
                }
            },
            goes_out_for: None,
            bearing: WORKER,
        },
        /// Sows wheat in good soil out in the wasteland, a field of
        /// [`PLOTS`](super::fields::PLOTS) crops, tends it every day, harvests
        /// it ripe and stores the wheat in its own grain bin at the farm
        /// (`FarmGoal`). Every day but Sunday.
        Farmer = 6 => JobDef {
            name: "farmer",
            article: "a farmer",
            workplace: FeatureKind::GrainBin,
            goal: GoalId::Farm,
            commute: COMMUTE,
            shifts: &[FARM_HOURS],
            uniform: "uniform farmer",
            now: |at_work, _| if at_work { "storing wheat" } else { "in the fields" },
            goes_out_for: Some(ItemKind::Wheat),
            bearing: WORKER,
        },
        /// Stands by at the power plant's control panel for one of the three
        /// [`WATCHES`] of the day, an hour at a time like a guard at a post
        /// (`WorkGoal`): the plant runs round the clock. There is nothing to
        /// do at the panel yet but be there.
        PowerPlantWorker = 7 => JobDef {
            name: "power plant worker",
            article: "a power plant worker",
            workplace: FeatureKind::ControlPanel,
            goal: GoalId::Work,
            commute: COMMUTE,
            shifts: &WATCHES,
            uniform: "uniform power plant worker",
            now: |at_work, _| if at_work { "on duty" } else { "on the way to the power plant" },
            goes_out_for: None,
            bearing: WORKER,
        },
        /// Holds court at the throne in the district's palace, an hour at a
        /// time like a guard at a post (`WorkGoal`), every day but Sunday —
        /// and nothing else yet: what a king decides is to come. One throne,
        /// so one king, hired last. In its robes and crown always, and
        /// walking at a stately pace ([`ROYAL`]).
        King = 8 => JobDef {
            name: "king",
            article: "the king",
            workplace: FeatureKind::Throne,
            goal: GoalId::Work,
            commute: KING_COMMUTE,
            shifts: &[COURT_HOURS],
            uniform: "uniform king",
            now: |at_work, _| if at_work { "holding court" } else { "on the way to the palace" },
            goes_out_for: None,
            bearing: ROYAL,
        },
        /// Repairs failed boxes across the map, one worker per watch at
        /// the public service box indexed by Features.
        Electrician = 9 => JobDef {
            name: "electrician",
            article: "an electrician",
            workplace: FeatureKind::DistributionBox,
            goal: GoalId::Repair,
            commute: COMMUTE,
            shifts: &WATCHES,
            uniform: "uniform electrician",
            now: |_, _| "checking for damaged boxes",
            goes_out_for: None,
            bearing: WORKER,
        },
    }
}

impl JobKind {
    pub const fn name(self) -> &'static str {
        self.def().name
    }

    /// "an office worker", "a scrap collector": for a log line.
    pub const fn with_article(self) -> &'static str {
        self.def().article
    }

    /// The feature the job is done at, and so what a workplace is handed out
    /// from.
    pub const fn workplace(self) -> FeatureKind {
        self.def().workplace
    }

    /// What its workplace is called, for a log line and the job menu: the
    /// prop it is.
    pub const fn workplace_name(self) -> &'static str {
        self.workplace().prop()
    }

    /// The goal the job is done by: what `WorkRoutine` raises while the
    /// shift is due. A guard post is stood at the way a desk is sat at.
    pub const fn goal(self) -> GoalId {
        self.def().goal
    }

    /// What somebody in this job wears to work (`GameEntity::worn`).
    pub const fn uniform(self) -> Clothing {
        Clothing::Uniform(self)
    }

    /// How long before its shift somebody in this job sets off, in **world**
    /// seconds: [`COMMUTE`], or [`GUARD_COMMUTE`] to a gate.
    pub const fn commute(self) -> f32 {
        self.def().commute
    }

    /// The shifts a workplace of this kind is worked in, **one worker to
    /// each**, in the order they are handed out: one for a desk or a bin,
    /// the three watches for a guard post.
    pub const fn shifts(self) -> &'static [Shift] {
        self.def().shifts
    }

    /// The job menu's `now` line for somebody at it ([`JobDef::now`]).
    pub fn now(self, at_work: bool, pack: Option<&Inventory>) -> &'static str {
        (self.def().now)(at_work, pack)
    }

    /// What a trip out brings back, for a job worked on trips out into the
    /// wasteland, whose worker gets ready before each ([`JobDef::goes_out_for`]).
    pub const fn goes_out_for(self) -> Option<ItemKind> {
        self.def().goes_out_for
    }

    /// How somebody in this job carries themselves ([`JobDef::bearing`]).
    pub const fn bearing(self) -> Bearing {
        self.def().bearing
    }
}

/// Monday to Friday, as a [`Shift::days`] mask.
pub const WEEKDAYS: u8 = 0b001_1111;

/// Every day of the week, as a [`Shift::days`] mask.
pub const EVERY_DAY: u8 = 0b111_1111;

/// Every day but Sunday, as a [`Shift::days`] mask.
pub const MONDAY_TO_SATURDAY: u8 = 0b011_1111;

/// Nine to five, Monday to Friday.
pub const OFFICE_HOURS: Shift = Shift {
    start: 9,
    end: 17,
    days: WEEKDAYS,
};

/// Nine to five, every day but Sunday: a field is tended every day it grows,
/// and a farmer has one day off.
pub const FARM_HOURS: Shift = Shift {
    start: 9,
    end: 17,
    days: MONDAY_TO_SATURDAY,
};

/// Court: ten to four, every day but Sunday. A king is up at seven and in bed
/// by eleven ([`Shift::is_bedtime`]).
pub const COURT_HOURS: Shift = Shift {
    start: 10,
    end: 16,
    days: MONDAY_TO_SATURDAY,
};

/// A guard post's three watches, every day: the day, the evening and the
/// night, in the order they are handed out. Between them, round the clock.
pub const WATCHES: [Shift; 3] = [
    Shift { start: 8, end: 16, days: EVERY_DAY },
    Shift { start: 16, end: 24, days: EVERY_DAY },
    Shift { start: 0, end: 8, days: EVERY_DAY },
];

/// How long before its shift starts a worker gets up: three hours, the
/// office worker's six o'clock for nine — long enough to see to the morning
/// and walk in, short enough that the end of the shift is not past
/// `EXHAUSTED` (eleven hours up, as for the office).
pub const UP_BEFORE_SHIFT: u8 = 3;

/// How long a worker's night is: the clock's, [`BEDTIME`] to [`GETTING_UP`].
const NIGHT_HOURS: u8 = (24 - BEDTIME + GETTING_UP) as u8;

/// Whether it is bedtime for somebody with `job` — the hours its brain
/// keeps to bed ([`Shift::is_bedtime`]) — or, with none, the clock's night.
pub fn is_bedtime(job: Option<&Job>, clock: Clock) -> bool {
    match job {
        Some(job) => job.shift.is_bedtime(clock),
        None => clock.is_night(),
    }
}

/// When a job is worked: from `start` to `end` o'clock **on the same day**,
/// on the days in `days`. A shift across midnight is two shifts — the
/// evening watch ends at 24 and the night watch starts at 0 — and
/// [`Shift::new`] refuses one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shift {
    start: u8,
    end: u8,
    /// Bit 0 is Monday, bit 6 Sunday.
    days: u8,
}

impl Shift {
    /// A shift, or `None` for one that ends before it starts, runs past
    /// midnight or is worked on no day.
    pub const fn new(start: u8, end: u8, days: u8) -> Option<Shift> {
        if start < end && end <= 24 && days != 0 && days < 0b1000_0000 {
            Some(Shift { start, end, days })
        } else {
            None
        }
    }

    pub const fn start(&self) -> u8 {
        self.start
    }

    pub const fn end(&self) -> u8 {
        self.end
    }

    pub const fn works_on(&self, day: Weekday) -> bool {
        self.days & (1 << day as u8) != 0
    }

    /// Whether it is the shift's hours on one of its days.
    pub fn is_working_hours(&self, clock: Clock) -> bool {
        let hours = clock.hours();
        self.works_on(clock.weekday()) && hours >= self.start as f32 && hours < self.end as f32
    }

    /// Whether a worker should be at work or on the way there: the shift's
    /// hours, and the [`COMMUTE`] before them.
    pub fn is_due(&self, clock: Clock) -> bool {
        self.is_due_after(clock, COMMUTE)
    }

    /// ...with `commute` world seconds before them ([`Job::is_due`], what
    /// `WorkRoutine` reads, passes the job's own). A shift from midnight is
    /// set off for the evening before, if it is worked the day after.
    pub fn is_due_after(&self, clock: Clock, commute: f32) -> bool {
        let hours = clock.hours();
        let setting_off = self.start as f32 - commute / HOUR;
        let today = self.works_on(clock.weekday()) && hours >= setting_off && hours < self.end as f32;
        let tonight = setting_off < 0.0 && hours >= 24.0 + setting_off && self.works_on(clock.weekday().next());
        today || tonight
    }

    /// Whether its worker should be in bed: the eight hours ([`BEDTIME`] to
    /// [`GETTING_UP`]'s length) that end [`UP_BEFORE_SHIFT`] hours before the
    /// shift starts, **every day**, worked or not, so a body keeps one clock.
    /// Office hours sleep the clock's night, ten to six; the night watch
    /// (from midnight) sleeps one to nine in the afternoon and evening, and
    /// is up three hours before it is due. What `SleepRoutine` reads as its
    /// night.
    pub fn is_bedtime(&self, clock: Clock) -> bool {
        let up = (self.start + 24 - UP_BEFORE_SHIFT) % 24;
        let down = (up + 24 - NIGHT_HOURS) % 24;
        let hour = clock.hour() as u8;
        if down < up {
            hour >= down && hour < up
        } else {
            hour >= down || hour < up
        }
    }

    /// `09:00-17:00`.
    pub fn hours(&self) -> String {
        format!("{:02}:00-{:02}:00", self.start, self.end)
    }

    /// `Mon-Fri`, `Mon-Sun`, or the days listed (`Mon, Wed, Sat`) when they
    /// are not one run.
    pub fn days(&self) -> String {
        let worked: Vec<Weekday> = Weekday::ALL.into_iter().filter(|&day| self.works_on(day)).collect();
        let (Some(first), Some(last)) = (worked.first(), worked.last()) else {
            return "never".to_string();
        };
        let run = (*last as u8 - *first as u8 + 1) as usize == worked.len();
        if worked.len() > 2 && run {
            format!("{}-{}", first.short(), last.short())
        } else {
            worked.iter().map(|day| day.short()).collect::<Vec<_>>().join(", ")
        }
    }
}

/// What somebody does for a living: the kind of job, the workplace it is
/// done at — **its own**, handed out with the job — and its hours.
///
/// The shift is the worker's, copied in when they are hired, so two people
/// in one job could keep different hours without anything here changing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Job {
    pub kind: JobKind,
    pub workplace: Point,
    pub shift: Shift,
}

impl Job {
    /// `kind` at `workplace`, keeping `shift`.
    pub const fn new(kind: JobKind, workplace: Point, shift: Shift) -> Job {
        Job { kind, workplace, shift }
    }

    /// An office worker at the computer in `desk`, keeping office hours.
    pub const fn office(desk: Point) -> Job {
        Job::new(JobKind::OfficeWorker, desk, OFFICE_HOURS)
    }

    /// A scrap collector storing in the bin in `bin`, keeping office hours.
    pub const fn collector(bin: Point) -> Job {
        Job::new(JobKind::ScrapCollector, bin, OFFICE_HOURS)
    }

    /// A guard at the post in `post`, keeping `watch` (one of [`WATCHES`]).
    pub const fn guard(post: Point, watch: Shift) -> Job {
        Job::new(JobKind::Guard, post, watch)
    }

    /// Whether its worker should be at work or on the way there: the
    /// shift's hours and the kind's [`JobKind::commute`] before them. What
    /// `WorkRoutine` reads.
    pub fn is_due(&self, clock: Clock) -> bool {
        self.shift.is_due_after(clock, self.kind.commute())
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Job {
        let Job { kind, workplace, shift: Shift { start, end, days } } = *self;
        pb::Job {
            kind: kind.number(),
            workplace: Some(workplace.into()),
            start: start as u32,
            end: end as u32,
            days: days as u32,
        }
    }

    pub(crate) fn restore(saved: &pb::Job) -> Result<Job, Invalid> {
        let Some(kind) = JobKind::from_number(saved.kind) else {
            return invalid(format!("no job numbered {}", saved.kind));
        };
        let workplace = pb::cell(saved.workplace, "workplace")?;
        let shift = match (u8::try_from(saved.start), u8::try_from(saved.end), u8::try_from(saved.days)) {
            (Ok(start), Ok(end), Ok(days)) => Shift::new(start, end, days),
            _ => None,
        };
        let Some(shift) = shift else {
            return invalid(format!("a shift from {} to {} on days {:#b}", saved.start, saved.end, saved.days));
        };
        Ok(Job { kind, workplace, shift })
    }
}

/// Which workplaces have been given to somebody, for which shift, and to
/// whom. The twin of [`Homes`](super::Homes), for the same reasons: a
/// `BTreeMap`, written only by the spawn pass, read by nothing in a tick.
///
/// **A workplace is a cell and a shift**, told apart by the hour the shift
/// starts: a desk or a bin has one (office hours), a guard post three (the
/// [`WATCHES`]), each given to somebody of its own.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Workplaces {
    workers: BTreeMap<(Point, u8), Uid>,
}

impl Workplaces {
    pub fn new() -> Workplaces {
        Workplaces::default()
    }

    /// Who works at `at` in `shift`, if anybody.
    pub fn worker(&self, at: Point, shift: Shift) -> Option<Uid> {
        self.workers.get(&(at, shift.start)).copied()
    }

    pub fn is_taken(&self, at: Point, shift: Shift) -> bool {
        self.workers.contains_key(&(at, shift.start))
    }

    /// The first of `shifts` nobody works `at` in, if any: the one an
    /// arrival given that workplace keeps.
    pub fn free_shift(&self, at: Point, shifts: &[Shift]) -> Option<Shift> {
        shifts.iter().copied().find(|&shift| !self.is_taken(at, shift))
    }

    /// Give `at` in `shift` to `uid`. `false`, and nothing changes, if
    /// somebody already works it: **a workplace is only ever given while it
    /// is free.**
    pub fn claim(&mut self, at: Point, shift: Shift, uid: Uid) -> bool {
        if self.is_taken(at, shift) {
            return false;
        }
        self.workers.insert((at, shift.start), uid);
        true
    }

    /// Take back whatever workplace `uid` had, and say where. A scan of the
    /// workplaces, once per despawn or dismissal, never per tick.
    pub fn release(&mut self, uid: Uid) -> Option<Point> {
        let key = self.workers.iter().find(|&(_, &worker)| worker == uid).map(|(&key, _)| key)?;
        self.workers.remove(&key);
        Some(key.0)
    }

    /// How many workplaces — a cell and a shift — have somebody working
    /// them.
    pub fn len(&self) -> usize {
        self.workers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.workers.is_empty()
    }

    // --- saving ---

    pub(crate) fn save(&self) -> Vec<pb::Workplace> {
        let Workplaces { workers } = self;
        workers
            .iter()
            .map(|(&(desk, start), &worker)| pb::Workplace {
                desk: Some(desk.into()),
                worker: worker.raw(),
                start: Some(start as u32),
            })
            .collect()
    }

    /// The workplaces as given out. One workplace to one worker, and one
    /// worker to a workplace. A save from before the watches says no start:
    /// office hours, all it could have held.
    pub(crate) fn restore(saved: &[pb::Workplace]) -> Result<Workplaces, Invalid> {
        let mut workplaces = Workplaces::new();
        for place in saved {
            let desk = pb::cell(place.desk, "desk")?;
            let Some(worker) = Uid::from_raw(place.worker) else {
                return invalid(format!("the desk at {}, {} is nobody's", desk.x, desk.y));
            };
            let start = match place.start.map(u8::try_from) {
                None => OFFICE_HOURS.start,
                Some(Ok(start)) if start < 24 => start,
                Some(_) => return invalid(format!("a shift at {}, {} starting at {:?}", desk.x, desk.y, place.start)),
            };
            if workplaces.workers.values().any(|&other| other == worker)
                || workplaces.workers.insert((desk, start), worker).is_some()
            {
                return invalid(format!("{worker} and the desk at {}, {} given twice", desk.x, desk.y));
            }
        }
        Ok(workplaces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::clock::{DAY, MINUTE, TIME_SCALE};
    use crate::sim::uid::EntityType;

    /// `hours` o'clock on day `day` (day 1 is a Monday).
    fn at(day: u32, hours: f32) -> Clock {
        let opening = 8.0 * HOUR as f64;
        let world = (day - 1) as f64 * DAY as f64 + hours as f64 * HOUR as f64 - opening;
        Clock::after_watching(world / TIME_SCALE as f64)
    }

    #[test]
    fn office_hours_are_nine_to_five_on_weekdays() {
        let shift = OFFICE_HOURS;
        assert!(!shift.is_working_hours(at(1, 8.99)));
        assert!(shift.is_working_hours(at(1, 9.0)));
        assert!(shift.is_working_hours(at(5, 16.99)), "Friday afternoon");
        assert!(!shift.is_working_hours(at(1, 17.0)), "five o'clock is home time");
        assert!(!shift.is_working_hours(at(6, 10.0)), "Saturday");
        assert!(!shift.is_working_hours(at(7, 10.0)), "Sunday");
        assert!(shift.is_working_hours(at(8, 10.0)), "and Monday again");
        assert_eq!(shift.hours(), "09:00-17:00");
        assert_eq!(shift.days(), "Mon-Fri");
    }

    /// The half hour on the way: due from half past eight, so the desk is
    /// reached by nine.
    #[test]
    fn a_shift_is_due_from_the_commute_before_it_until_it_ends() {
        let shift = OFFICE_HOURS;
        let commute_starts = 9.0 - COMMUTE / HOUR;
        assert!(!shift.is_due(at(1, commute_starts - MINUTE / HOUR)));
        assert!(shift.is_due(at(1, commute_starts + MINUTE / HOUR)));
        assert!(!shift.is_working_hours(at(1, commute_starts + MINUTE / HOUR)), "on the way, not at work yet");
        assert!(shift.is_due(at(3, 12.0)));
        assert!(!shift.is_due(at(3, 17.0)));
        assert!(!shift.is_due(at(6, 8.75)), "nobody sets off on a Saturday");
    }

    #[test]
    fn a_shift_that_ends_before_it_starts_or_runs_on_no_day_is_refused() {
        assert_eq!(Shift::new(17, 9, WEEKDAYS), None);
        assert_eq!(Shift::new(9, 9, WEEKDAYS), None);
        assert_eq!(Shift::new(9, 25, WEEKDAYS), None);
        assert_eq!(Shift::new(9, 17, 0), None);
        assert_eq!(Shift::new(9, 17, WEEKDAYS), Some(OFFICE_HOURS));
    }

    #[test]
    fn days_that_are_not_one_run_are_listed() {
        let odd = Shift::new(9, 12, 0b010_0101).unwrap();
        assert_eq!(odd.days(), "Mon, Wed, Sat");
        let weekend = Shift::new(9, 12, 0b110_0000).unwrap();
        assert_eq!(weekend.days(), "Sat, Sun");
    }

    /// A collector keeps office hours, at a bin, by its own goal; and the
    /// desks are offered before the bins.
    #[test]
    fn a_scrap_collector_works_office_hours_at_a_bin_and_is_hired_after_the_office() {
        let kind = JobKind::ScrapCollector;
        assert_eq!((kind.name(), kind.with_article()), ("scrap collector", "a scrap collector"));
        assert_eq!((kind.workplace(), kind.workplace_name()), (FeatureKind::ScrapBin, "scrap bin"));
        assert_eq!(kind.goal(), GoalId::Collect);
        assert_eq!(kind.shifts(), [OFFICE_HOURS]);
        assert_eq!(JobKind::OfficeWorker.goal(), GoalId::Work);
        assert_eq!(JobKind::OfficeWorker.with_article(), "an office worker");
        assert_eq!(
            JobKind::ALL,
            [
                JobKind::OfficeWorker,
                JobKind::ScrapCollector,
                JobKind::Guard,
                JobKind::Undertaker,
                JobKind::Steelworker,
                JobKind::Courier,
                JobKind::Farmer,
                JobKind::PowerPlantWorker,
                JobKind::King,
                JobKind::Electrician
            ],
            "the office first, the electrician last"
        );
        let job = Job::collector(Point::new(-4, 9));
        assert_eq!((job.kind, job.workplace, job.shift), (kind, Point::new(-4, 9), OFFICE_HOURS));
        assert_eq!(Job::restore(&job.save()), Ok(job));
    }

    #[test]
    fn a_job_comes_back_from_a_save_as_it_was_and_a_broken_shift_is_refused() {
        let job = Job::office(Point::new(12, -3));
        assert_eq!(Job::restore(&job.save()), Ok(job));
        let mut broken = job.save();
        broken.end = 3;
        assert!(Job::restore(&broken).is_err());
        broken = job.save();
        broken.kind = 99;
        assert!(Job::restore(&broken).is_err());
    }

    fn human(n: u64) -> Uid {
        Uid::new(EntityType::Human, n)
    }

    #[test]
    fn a_desk_is_given_to_one_worker_and_is_free_again_when_they_go() {
        let mut places = Workplaces::new();
        let desk = Point::new(3, 4);
        assert!(places.claim(desk, OFFICE_HOURS, human(1)));
        assert!(!places.claim(desk, OFFICE_HOURS, human(2)), "somebody works there");
        assert_eq!(places.worker(desk, OFFICE_HOURS), Some(human(1)));
        assert_eq!(places.release(human(2)), None, "nothing to give back");
        assert_eq!(places.release(human(1)), Some(desk));
        assert!(!places.is_taken(desk, OFFICE_HOURS));
        assert!(places.claim(desk, OFFICE_HOURS, human(2)));
    }

    #[test]
    fn desks_come_back_from_a_save_and_one_given_twice_is_refused() {
        let mut places = Workplaces::new();
        places.claim(Point::new(1, 1), OFFICE_HOURS, human(1));
        places.claim(Point::new(2, 1), OFFICE_HOURS, human(2));
        places.claim(Point::new(2, 1), WATCHES[2], human(3));
        assert_eq!(Workplaces::restore(&places.save()), Ok(places.clone()));

        let mut older = places.save();
        older.retain(|place| place.start == Some(OFFICE_HOURS.start as u32));
        for place in &mut older {
            place.start = None;
        }
        let restored = Workplaces::restore(&older).unwrap();
        assert_eq!(restored.worker(Point::new(2, 1), OFFICE_HOURS), Some(human(2)), "no start is office hours");

        let mut twice = places.save();
        twice[1].worker = human(1).raw();
        assert!(Workplaces::restore(&twice).is_err(), "one worker, two desks");
        let mut twice = places.save();
        twice[1].desk = twice[0].desk;
        twice[1].start = twice[0].start;
        assert!(Workplaces::restore(&twice).is_err(), "one desk, two workers");
        let mut watches = places.save();
        watches[1].desk = watches[0].desk;
        assert!(Workplaces::restore(&watches).is_ok(), "one post, two watches");
        let mut late = places.save();
        late[0].start = Some(24);
        assert!(Workplaces::restore(&late).is_err(), "no shift starts at midnight's end");
    }

    /// A guard post is three workplaces: each watch is somebody's, and the
    /// three between them are the whole day, every day.
    #[test]
    fn a_guard_post_is_kept_round_the_clock_by_three_watches_every_day() {
        let kind = JobKind::Guard;
        assert_eq!((kind.name(), kind.with_article()), ("guard", "a guard"));
        assert_eq!((kind.workplace(), kind.workplace_name()), (FeatureKind::GuardPost, "guard post"));
        assert_eq!(kind.goal(), GoalId::Work, "stood at like a desk");
        assert_eq!(kind.shifts(), WATCHES);
        for day in 1..=7 {
            for quarter in 0..96 {
                let clock = at(day, quarter as f32 / 4.0 + 0.01);
                let on = WATCHES.iter().filter(|watch| watch.is_working_hours(clock)).count();
                assert_eq!(on, 1, "day {day} at {:.2}", clock.hours());
            }
        }
        assert_eq!(WATCHES.map(|watch| watch.hours()), ["08:00-16:00", "16:00-24:00", "00:00-08:00"]);
        assert_eq!(WATCHES[0].days(), "Mon-Sun");

        let post = Point::new(0, 20);
        let mut places = Workplaces::new();
        assert_eq!(places.free_shift(post, kind.shifts()), Some(WATCHES[0]));
        for (n, &watch) in WATCHES.iter().enumerate() {
            assert!(places.claim(post, watch, human(n as u64 + 1)));
        }
        assert_eq!(places.free_shift(post, kind.shifts()), None, "three guards to a post");
        assert_eq!(places.len(), 3);
        assert_eq!(places.release(human(2)), Some(post));
        assert_eq!(places.free_shift(post, kind.shifts()), Some(WATCHES[1]), "the evening is free again");
        let job = Job::guard(post, WATCHES[2]);
        assert_eq!(Job::restore(&job.save()), Ok(job));
    }

    /// A power plant worker stands by at the plant's control panel like a
    /// guard at a post: the work goal, the three watches, a worker to each,
    /// hired last; on duty at the panel, on the way there otherwise.
    #[test]
    fn a_power_plant_worker_keeps_the_control_panel_round_the_clock_and_is_hired_before_the_king() {
        let kind = JobKind::PowerPlantWorker;
        assert_eq!((kind.name(), kind.with_article()), ("power plant worker", "a power plant worker"));
        assert_eq!((kind.workplace(), kind.workplace_name()), (FeatureKind::ControlPanel, "control panel"));
        assert_eq!(kind.goal(), GoalId::Work, "stood at like a post");
        assert_eq!(kind.shifts(), WATCHES);
        assert_eq!(JobKind::ALL[JobKind::COUNT - 3], kind);
        assert_eq!((kind.now(true, None), kind.now(false, None)), ("on duty", "on the way to the power plant"));
        assert_eq!(kind.uniform().name(), "uniform power plant worker");

        let panel = Point::new(30, 4);
        let mut places = Workplaces::new();
        for (n, &watch) in WATCHES.iter().enumerate() {
            assert_eq!(places.free_shift(panel, kind.shifts()), Some(watch));
            assert!(places.claim(panel, watch, human(n as u64 + 1)));
        }
        assert_eq!(places.free_shift(panel, kind.shifts()), None, "three workers to a panel");
        let job = Job::new(kind, panel, WATCHES[1]);
        assert_eq!(Job::restore(&job.save()), Ok(job));
    }

    /// The king: the palace's throne, held court at like a post, ten to four
    /// every day but Sunday, hired last, one to a throne; robes and crown
    /// always on, and a stately walk — and nobody else carries themselves so.
    #[test]
    fn the_king_holds_court_at_its_throne_in_its_crown_and_walks_slowly() {
        let kind = JobKind::King;
        assert_eq!((kind.name(), kind.with_article()), ("king", "the king"));
        assert_eq!((kind.workplace(), kind.workplace_name()), (FeatureKind::Throne, "throne"));
        assert_eq!(kind.goal(), GoalId::Work, "stood at like a post, until there are decisions to make");
        assert_eq!(kind.shifts(), [COURT_HOURS]);
        assert_eq!(JobKind::ALL[JobKind::COUNT - 2], kind);
        assert_eq!((kind.now(true, None), kind.now(false, None)), ("holding court", "on the way to the palace"));
        assert_eq!(kind.uniform().name(), "uniform king");
        assert_eq!(kind.bearing(), ROYAL);
        assert!(ROYAL.gait < 1.0 && ROYAL.always_in_uniform && ROYAL.hat == Some("crown"));
        assert!(JobKind::ALL.into_iter().filter(|&other| other != kind).all(|other| other.bearing() == WORKER));

        let throne = Point::new(10, 4);
        let mut places = Workplaces::new();
        assert_eq!(places.free_shift(throne, kind.shifts()), Some(COURT_HOURS));
        assert!(places.claim(throne, COURT_HOURS, human(1)));
        assert_eq!(places.free_shift(throne, kind.shifts()), None, "one king to a throne");
        let job = Job::new(kind, throne, COURT_HOURS);
        assert_eq!(Job::restore(&job.save()), Ok(job));

        assert!(!job.is_due(at(1, 8.4)) && job.is_due(at(1, 8.6)), "sets off at half past eight");
        assert!(job.is_due(at(6, 15.9)) && !job.is_due(at(7, 11.0)), "Saturday, and not Sunday");
        assert!(COURT_HOURS.is_bedtime(at(1, 23.5)) && COURT_HOURS.is_bedtime(at(2, 6.5)), "in bed eleven to seven");
        assert!(!COURT_HOURS.is_bedtime(at(1, 22.5)) && !COURT_HOURS.is_bedtime(at(2, 7.5)));
    }

    /// The night watch is due from the evening before — two hours before
    /// it, a guard's commute — as the office is from half past eight.
    #[test]
    fn a_watch_from_midnight_is_set_off_for_the_evening_before() {
        let night = WATCHES[2];
        assert!(!night.is_due(at(1, 23.4)));
        assert!(night.is_due(at(1, 23.6)), "on the way");
        assert!(!night.is_working_hours(at(1, 23.6)));
        assert!(night.is_due(at(2, 0.5)));
        assert!(night.is_due(at(2, 7.9)));
        assert!(!night.is_due(at(2, 8.0)));
        let guard = Job::guard(Point::new(0, 0), night);
        assert!(!guard.is_due(at(1, 21.9)));
        assert!(guard.is_due(at(1, 22.1)), "a guard sets off two hours before its watch");
        assert!(!guard.is_due(at(2, 8.0)));
        assert!(Job::guard(Point::new(0, 0), WATCHES[0]).is_due(at(1, 6.1)), "the day watch at six");
        assert!(!Job::office(Point::new(0, 0)).is_due(at(1, 8.4)), "the office still at half past eight");
        let weekdays = Shift::new(0, 8, WEEKDAYS).unwrap();
        assert!(weekdays.is_due(at(7, 23.6)), "Monday's watch is set off for on Sunday evening");
        assert!(!weekdays.is_due(at(5, 23.6)), "and there is no Saturday's to set off for on Friday");
    }

    /// Bed is the eight hours ending three before the shift: the clock's own
    /// night for office hours, the afternoon for the night watch.
    #[test]
    fn a_worker_sleeps_the_eight_hours_ending_three_before_its_shift() {
        for hour in 0..24 {
            let clock = at(2, hour as f32 + 0.5);
            assert_eq!(OFFICE_HOURS.is_bedtime(clock), clock.is_night(), "office hours keep the clock's night, {hour}:30");
            assert_eq!(is_bedtime(None, clock), clock.is_night(), "and so does nobody");
        }
        let in_bed = |shift: Shift| (0..24).filter(|&hour| shift.is_bedtime(at(2, hour as f32 + 0.5))).collect::<Vec<_>>();
        assert_eq!(in_bed(WATCHES[0]), [0, 1, 2, 3, 4, 21, 22, 23], "the day watch: nine to five in the morning");
        assert_eq!(in_bed(WATCHES[1]), [5, 6, 7, 8, 9, 10, 11, 12], "the evening watch: five to one");
        assert_eq!(in_bed(WATCHES[2]), [13, 14, 15, 16, 17, 18, 19, 20], "the night watch: one to nine");
        let job = Job::guard(Point::new(0, 0), WATCHES[2]);
        assert!(is_bedtime(Some(&job), at(2, 15.0)));
        assert!(!is_bedtime(Some(&job), at(2, 2.0)), "on watch at two in the morning");
    }
}
