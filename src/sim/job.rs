//! Jobs: what somebody does for a living, where, and when — [`Job`] — and
//! [`Workplaces`], which desks have been given to whom.
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
//! Two jobs, both nine to five, Monday to Friday, offered in this order
//! ([`JobKind::ALL`]):
//!
//! * the **office worker**: a computer, worked an hour at a time
//!   (`WorkGoal`);
//! * the **scrap collector**: a scrap bin in the warehouse, and the heaps of
//!   scrap out in the wasteland — out to a heap, salvage a packful, back to
//!   the bin and store it (`CollectGoal`). One collector to a bin.

use std::collections::BTreeMap;

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::brain::GoalId;
use super::clock::{Clock, Weekday, HOUR};
use super::feature::FeatureKind;
use super::uid::Uid;

/// How long before a shift starts a worker sets off for it, in **world**
/// seconds: half an hour, so somebody across the district is at the desk by
/// the hour rather than leaving home at it.
pub const COMMUTE: f32 = 0.5 * HOUR;

/// Every kind of job there is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobKind {
    /// Sits at a computer and works it. Tiring, and no fun.
    OfficeWorker,
    /// Brings scrap metal in from the heaps in the wasteland and stores it
    /// in its own bin in the warehouse. As tiring as office work, and as
    /// little fun.
    ScrapCollector,
}

impl JobKind {
    /// Every kind, **in the order a job is offered to an arrival**
    /// (`GameState::give_a_job`): the bank's desks first, then the bins.
    pub const ALL: [JobKind; 2] = [JobKind::OfficeWorker, JobKind::ScrapCollector];

    pub const fn name(self) -> &'static str {
        match self {
            JobKind::OfficeWorker => "office worker",
            JobKind::ScrapCollector => "scrap collector",
        }
    }

    /// "an office worker", "a scrap collector": for a log line.
    pub const fn with_article(self) -> &'static str {
        match self {
            JobKind::OfficeWorker => "an office worker",
            JobKind::ScrapCollector => "a scrap collector",
        }
    }

    /// The feature the job is done at, and so what a workplace is handed out
    /// from.
    pub const fn workplace(self) -> FeatureKind {
        match self {
            JobKind::OfficeWorker => FeatureKind::Workstation,
            JobKind::ScrapCollector => FeatureKind::ScrapBin,
        }
    }

    /// What its workplace is called, for a log line and the job menu.
    pub const fn workplace_name(self) -> &'static str {
        match self {
            JobKind::OfficeWorker => "computer",
            JobKind::ScrapCollector => "scrap bin",
        }
    }

    /// The goal the job is done by: what `WorkRoutine` raises while the
    /// shift is due.
    pub const fn goal(self) -> GoalId {
        match self {
            JobKind::OfficeWorker => GoalId::Work,
            JobKind::ScrapCollector => GoalId::Collect,
        }
    }

    /// The hours somebody hired for it keeps.
    pub const fn shift(self) -> Shift {
        match self {
            JobKind::OfficeWorker | JobKind::ScrapCollector => OFFICE_HOURS,
        }
    }
}

impl From<JobKind> for pb::JobKind {
    fn from(kind: JobKind) -> pb::JobKind {
        match kind {
            JobKind::OfficeWorker => pb::JobKind::OfficeWorker,
            JobKind::ScrapCollector => pb::JobKind::ScrapCollector,
        }
    }
}

impl From<pb::JobKind> for JobKind {
    fn from(kind: pb::JobKind) -> JobKind {
        match kind {
            pb::JobKind::OfficeWorker => JobKind::OfficeWorker,
            pb::JobKind::ScrapCollector => JobKind::ScrapCollector,
        }
    }
}

/// Monday to Friday, as a [`Shift::days`] mask.
pub const WEEKDAYS: u8 = 0b001_1111;

/// Nine to five, Monday to Friday.
pub const OFFICE_HOURS: Shift = Shift {
    start: 9,
    end: 17,
    days: WEEKDAYS,
};

/// When a job is worked: from `start` to `end` o'clock **on the same day**,
/// on the days in `days`. A shift across midnight is not something any job
/// has yet, and [`Shift::new`] refuses one.
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
    /// hours, and the [`COMMUTE`] before them. What `WorkRoutine` reads.
    pub fn is_due(&self, clock: Clock) -> bool {
        let hours = clock.hours();
        self.works_on(clock.weekday()) && hours >= self.start as f32 - COMMUTE / HOUR && hours < self.end as f32
    }

    /// `09:00-17:00`.
    pub fn hours(&self) -> String {
        format!("{:02}:00-{:02}:00", self.start, self.end)
    }

    /// `Mon-Fri`, or the days listed (`Mon, Wed, Sat`) when they are not one
    /// run.
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
    /// `kind` at `workplace`, keeping the kind's hours.
    pub const fn new(kind: JobKind, workplace: Point) -> Job {
        Job {
            kind,
            workplace,
            shift: kind.shift(),
        }
    }

    /// An office worker at the computer in `desk`, keeping office hours.
    pub const fn office(desk: Point) -> Job {
        Job::new(JobKind::OfficeWorker, desk)
    }

    /// A scrap collector storing in the bin in `bin`, keeping office hours.
    pub const fn collector(bin: Point) -> Job {
        Job::new(JobKind::ScrapCollector, bin)
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Job {
        let Job { kind, workplace, shift: Shift { start, end, days } } = *self;
        pb::Job {
            kind: pb::JobKind::from(kind) as i32,
            workplace: Some(workplace.into()),
            start: start as u32,
            end: end as u32,
            days: days as u32,
        }
    }

    pub(crate) fn restore(saved: &pb::Job) -> Result<Job, Invalid> {
        let kind = JobKind::from(pb::known::<pb::JobKind>(saved.kind, "job")?);
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

/// Which workplaces have been given to somebody, and to whom. The twin of
/// [`Homes`](super::Homes), for the same reasons: a `BTreeMap`, written only
/// by the spawn pass, read by nothing in a tick.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Workplaces {
    workers: BTreeMap<Point, Uid>,
}

impl Workplaces {
    pub fn new() -> Workplaces {
        Workplaces::default()
    }

    /// Who works at `desk`, if anybody.
    pub fn worker(&self, desk: Point) -> Option<Uid> {
        self.workers.get(&desk).copied()
    }

    pub fn is_taken(&self, desk: Point) -> bool {
        self.workers.contains_key(&desk)
    }

    /// Give `desk` to `uid`. `false`, and nothing changes, if somebody
    /// already works there: **a desk is only ever given while it is free.**
    pub fn claim(&mut self, desk: Point, uid: Uid) -> bool {
        if self.is_taken(desk) {
            return false;
        }
        self.workers.insert(desk, uid);
        true
    }

    /// Take back whatever desk `uid` had, and say which. A scan of the desks,
    /// once per despawn or dismissal, never per tick.
    pub fn release(&mut self, uid: Uid) -> Option<Point> {
        let desk = self.workers.iter().find(|&(_, &worker)| worker == uid).map(|(&desk, _)| desk)?;
        self.workers.remove(&desk);
        Some(desk)
    }

    /// How many desks have somebody working at them.
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
            .map(|(&desk, &worker)| pb::Workplace { desk: Some(desk.into()), worker: worker.raw() })
            .collect()
    }

    /// The desks as given out. One desk to one worker, and one worker to a
    /// desk.
    pub(crate) fn restore(saved: &[pb::Workplace]) -> Result<Workplaces, Invalid> {
        let mut workplaces = Workplaces::new();
        for place in saved {
            let desk = pb::cell(place.desk, "desk")?;
            let Some(worker) = Uid::from_raw(place.worker) else {
                return invalid(format!("the desk at {}, {} is nobody's", desk.x, desk.y));
            };
            if workplaces.workers.values().any(|&other| other == worker) || !workplaces.claim(desk, worker) {
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
        assert_eq!(kind.shift(), OFFICE_HOURS);
        assert_eq!(JobKind::OfficeWorker.goal(), GoalId::Work);
        assert_eq!(JobKind::OfficeWorker.with_article(), "an office worker");
        assert_eq!(JobKind::ALL, [JobKind::OfficeWorker, JobKind::ScrapCollector], "the office first");
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
        assert!(places.claim(desk, human(1)));
        assert!(!places.claim(desk, human(2)), "somebody works there");
        assert_eq!(places.worker(desk), Some(human(1)));
        assert_eq!(places.release(human(2)), None, "nothing to give back");
        assert_eq!(places.release(human(1)), Some(desk));
        assert!(!places.is_taken(desk));
        assert!(places.claim(desk, human(2)));
    }

    #[test]
    fn desks_come_back_from_a_save_and_one_given_twice_is_refused() {
        let mut places = Workplaces::new();
        places.claim(Point::new(1, 1), human(1));
        places.claim(Point::new(2, 1), human(2));
        assert_eq!(Workplaces::restore(&places.save()), Ok(places.clone()));

        let mut twice = places.save();
        twice[1].worker = human(1).raw();
        assert!(Workplaces::restore(&twice).is_err(), "one worker, two desks");
        let mut twice = places.save();
        twice[1].desk = twice[0].desk;
        assert!(Workplaces::restore(&twice).is_err(), "one desk, two workers");
    }
}
