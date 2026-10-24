//! [`Timesheet`]: what a unit is owed for — the shifts it was on duty for and
//! the days it went by out of work — kept by its own brain, and handed to the
//! world's books ([`Effect::Record`]) for a clerk to pay
//! ([`crate::sim::economy`]).
//!
//! **A shift worked is paid in full** if the unit was on duty at all: its
//! job's goal took charge while the shift was due, the walk there included.
//! It is recorded when the shift is over — it is no longer due, or the job
//! changed or went. A job on call ([`JobKind::is_on_call`]: the
//! electrician's, which goes out only when a box fails) is on duty for every
//! shift it is employed through. A shift never gone to is not paid.
//!
//! **A care package a day** for the out of work: each day that turns with no
//! job adds a day owed.
//!
//! The brain asks the world for what it is owed once, with the first tick its
//! effect is free ([`Brain::react`](super::Brain::react), step 6c): a task
//! finishing, or an administrator running the playzone, comes first, and the
//! record waits for the next tick.

use crate::save::{invalid, pb, Invalid};
use crate::sim::clock::Clock;
use crate::sim::job::{Job, JobKind};
use crate::sim::Effect;

use super::GoalId;

/// See the [module docs](self). Eight bytes, in every human's brain.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Timesheet {
    /// The day it last looked at the clock: when the next one turns, a day
    /// out of work is owed. 0 before it has looked — just arrived, or loaded
    /// from a save from before there was money — which owes nothing.
    day: u32,
    /// The job it is on duty for right now, if any: set when the job's goal
    /// takes charge while the shift is due, kept until the shift is over.
    clocked: Option<JobKind>,
    /// Shifts over and not yet handed to the world...
    shifts: u8,
    /// ...and the job they were worked as.
    shift_job: Option<JobKind>,
    /// Days gone by out of work, not yet handed to the world.
    care_days: u8,
}

impl Timesheet {
    pub const fn new() -> Timesheet {
        Timesheet { day: 0, clocked: None, shifts: 0, shift_job: None, care_days: 0 }
    }

    /// Bring it up to date at `clock`, with the unit's `job` and the goal
    /// `top` in charge after the routines and the goal have had their say.
    pub fn tick(&mut self, job: Option<&Job>, clock: Clock, top: GoalId) {
        // A shift over — no longer due, or the job changed or went: it was on
        // duty for it, so the whole of it is owed. Two shifts as two jobs
        // waiting at once (a record held off while the unit was let go and
        // hired as something else) are both paid as the second.
        if let Some(kind) = self.clocked
            && !job.is_some_and(|job| job.kind == kind && job.is_due(clock))
        {
            self.shifts = self.shifts.saturating_add(1);
            self.shift_job = Some(kind);
            self.clocked = None;
        }
        if self.clocked.is_none()
            && let Some(job) = job
            && job.is_due(clock)
            && (top == job.kind.goal() || job.kind.is_on_call())
        {
            self.clocked = Some(job.kind);
        }
        let today = clock.day();
        if self.day != 0 && today > self.day && job.is_none() {
            self.care_days = self.care_days.saturating_add((today - self.day).min(u8::MAX as u32) as u8);
        }
        self.day = self.day.max(today);
    }

    /// Whether anything is waiting to be handed to the world.
    pub fn is_pending(&self) -> bool {
        self.shifts > 0 || self.care_days > 0
    }

    /// What is waiting, as the effect that hands it to the world — and
    /// forgotten here. `None` with nothing waiting.
    pub fn take(&mut self) -> Option<Effect> {
        if !self.is_pending() {
            return None;
        }
        let effect = Effect::Record { shifts: self.shifts, job: self.shift_job, care_days: self.care_days };
        self.shifts = 0;
        self.shift_job = None;
        self.care_days = 0;
        Some(effect)
    }

    /// The job it is on duty for right now, if any.
    pub fn on_duty(&self) -> Option<JobKind> {
        self.clocked
    }

    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let on_duty = self.clocked.map_or("no".to_string(), |kind| format!("as {}", kind.with_article()));
        vec![("on duty", on_duty), ("unrecorded", format!("{} shifts, {} care days", self.shifts, self.care_days))]
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Timesheet {
        let Timesheet { day, clocked, shifts, shift_job, care_days } = *self;
        pb::Timesheet {
            day,
            clocked: clocked.map(JobKind::number),
            shifts: shifts as u32,
            shift_job: shift_job.map(JobKind::number),
            care_days: care_days as u32,
        }
    }

    /// A timesheet as saved; absent — a save from before there was money —
    /// is a fresh one, which owes nothing until it has looked at the clock.
    pub(crate) fn restore(saved: Option<&pb::Timesheet>) -> Result<Timesheet, Invalid> {
        let Some(saved) = saved else {
            return Ok(Timesheet::new());
        };
        let kind = |number: Option<i32>| match number {
            None => Ok(None),
            Some(number) => JobKind::from_number(number)
                .map(Some)
                .ok_or_else(|| Invalid::new(format!("a timesheet for no job numbered {number}"))),
        };
        let (Ok(shifts), Ok(care_days)) = (u8::try_from(saved.shifts), u8::try_from(saved.care_days)) else {
            return invalid(format!("a timesheet of {} shifts and {} care days", saved.shifts, saved.care_days));
        };
        Ok(Timesheet { day: saved.day, clocked: kind(saved.clocked)?, shifts, shift_job: kind(saved.shift_job)?, care_days })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Point;
    use crate::sim::clock::{DAY, HOUR, TIME_SCALE};
    use crate::sim::job::WATCHES;

    /// `hours` o'clock on day `day` (day 1 is a Monday).
    fn at(day: u32, hours: f32) -> Clock {
        let world = (day - 1) as f64 * DAY as f64 + (hours as f64 - 8.0) * HOUR as f64;
        Clock::after_watching(world / TIME_SCALE as f64)
    }

    fn office() -> Job {
        Job::office(Point::new(3, 3))
    }

    /// Tick every quarter hour from `from` to `to` on `day`, with `top` in
    /// charge while `on_duty` says so and wandering otherwise.
    fn run(sheet: &mut Timesheet, job: Option<&Job>, day: u32, from: f32, to: f32, on_duty: impl Fn(f32) -> bool) {
        let mut hours = from;
        while hours < to {
            let top = if on_duty(hours) { job.map_or(GoalId::Wander, |job| job.kind.goal()) } else { GoalId::Wander };
            sheet.tick(job, at(day, hours), top);
            hours += 0.25;
        }
    }

    #[test]
    fn a_shift_on_duty_is_recorded_when_it_is_over() {
        let job = office();
        let mut sheet = Timesheet::new();
        run(&mut sheet, Some(&job), 1, 8.0, 12.0, |_| true);
        assert_eq!(sheet.on_duty(), Some(JobKind::OfficeWorker), "clocked in on the way");
        assert!(!sheet.is_pending(), "not over yet");
        run(&mut sheet, Some(&job), 1, 12.0, 18.0, |_| true);
        assert_eq!(sheet.on_duty(), None);
        assert_eq!(sheet.take(), Some(Effect::Record { shifts: 1, job: Some(JobKind::OfficeWorker), care_days: 0 }));
        assert_eq!(sheet.take(), None, "handed over once");
    }

    #[test]
    fn a_shift_never_gone_to_is_not_recorded() {
        let job = office();
        let mut sheet = Timesheet::new();
        run(&mut sheet, Some(&job), 1, 8.0, 18.0, |_| false);
        assert!(!sheet.is_pending());
        run(&mut sheet, Some(&job), 6, 8.0, 18.0, |_| true);
        assert!(!sheet.is_pending(), "nor a Saturday, which is not a shift");
    }

    /// On duty at all is the whole shift: an hour at the end of the day is
    /// paid as a day.
    #[test]
    fn a_part_shift_is_paid_as_a_whole_one() {
        let job = office();
        let mut sheet = Timesheet::new();
        run(&mut sheet, Some(&job), 1, 8.0, 18.0, |hours| hours >= 16.0);
        assert_eq!(sheet.take(), Some(Effect::Record { shifts: 1, job: Some(JobKind::OfficeWorker), care_days: 0 }));
    }

    #[test]
    fn being_let_go_mid_shift_records_the_shift() {
        let job = Job::guard(Point::new(0, 0), WATCHES[0]);
        let mut sheet = Timesheet::new();
        run(&mut sheet, Some(&job), 1, 8.0, 11.0, |_| true);
        sheet.tick(None, at(1, 11.0), GoalId::Wander);
        assert_eq!(sheet.take(), Some(Effect::Record { shifts: 1, job: Some(JobKind::Guard), care_days: 0 }));
    }

    #[test]
    fn every_day_out_of_work_is_a_care_day_and_a_working_day_is_not() {
        let mut sheet = Timesheet::new();
        run(&mut sheet, None, 1, 8.0, 24.0, |_| false);
        assert!(!sheet.is_pending(), "the day it arrived is not owed");
        run(&mut sheet, None, 2, 0.0, 24.0, |_| false);
        run(&mut sheet, None, 3, 0.0, 1.0, |_| false);
        assert_eq!(sheet.take(), Some(Effect::Record { shifts: 0, job: None, care_days: 2 }), "one a day turned");
        let job = office();
        run(&mut sheet, Some(&job), 4, 0.0, 1.0, |_| false);
        assert!(!sheet.is_pending(), "a day turned in work is no care day");
    }

    /// An electrician is on call: paid for every watch it is employed
    /// through, whatever is in charge.
    #[test]
    fn an_electrician_on_call_is_paid_for_every_watch() {
        let job = Job::new(JobKind::Electrician, Point::new(0, 0), WATCHES[0]);
        let mut sheet = Timesheet::new();
        run(&mut sheet, Some(&job), 1, 8.0, 17.0, |_| false);
        assert_eq!(sheet.take(), Some(Effect::Record { shifts: 1, job: Some(JobKind::Electrician), care_days: 0 }));
    }

    #[test]
    fn a_timesheet_comes_back_from_a_save_as_it_was() {
        let job = office();
        let mut sheet = Timesheet::new();
        run(&mut sheet, Some(&job), 1, 8.0, 18.0, |_| true);
        run(&mut sheet, Some(&job), 2, 8.0, 10.0, |_| true);
        assert!(sheet.is_pending() && sheet.on_duty().is_some());
        assert_eq!(Timesheet::restore(Some(&sheet.save())), Ok(sheet));
        assert_eq!(Timesheet::restore(None), Ok(Timesheet::new()));
        let mut broken = sheet.save();
        broken.clocked = Some(99);
        assert!(Timesheet::restore(Some(&broken)).is_err());
    }
}
