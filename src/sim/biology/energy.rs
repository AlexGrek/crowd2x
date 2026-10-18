//! [`Energy`]: tired with time awake, rested by sleep.
//!
//! Drives [`Stats::stamina`], which has been in every body since the stats
//! were first rolled and which nothing moved until now. Like [`Fun`](super::Fun)
//! it is a stat that *drains* on its own, and what the routine watching it
//! reads is [`Stats::tiredness`] — the same number the other way up, so every
//! need is still on one scale.
//!
//! What restores it is [`Event::Slept`], which says **how long** somebody was
//! in bed and not how much rest that is: the same split as an item saying what
//! it is made of and hunger deciding what that does. What wears it down faster
//! is [`Event::Worked`], the same way round: how long at the desk, and this
//! process says what a working hour costs.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// World hours from fully rested to exhausted: a waking day.
pub const HOURS_TO_EXHAUSTED: f32 = 16.0;

/// Stamina lost per **world** second — [`Think::game_dt`], never `dt`.
///
/// [`Think::game_dt`]: crate::sim::entity::Think::game_dt
pub const ENERGY_PER_SECOND: f32 = 100.0 / (HOURS_TO_EXHAUSTED * HOUR);

/// World hours in bed that undo a whole waking day.
pub const HOURS_OF_SLEEP: f32 = 8.0;

/// Stamina gained per world second slept.
///
/// The drain keeps running while somebody is asleep — this process keeps its
/// whole pace in bed ([`Process::asleep_pace`] is left at 1) — so it is added
/// back here: the *net* gain is `100 / HOURS_OF_SLEEP` an hour, and eight hours
/// in bed is a full recovery.
pub const RECOVERY_PER_SECOND: f32 = 100.0 / (HOURS_OF_SLEEP * HOUR) + ENERGY_PER_SECOND;

/// How many waking hours one hour of work tires a body like.
///
/// **Only a little over one, on purpose.** A day already runs close to the
/// line: somebody up rested at six is 68.75 tired by five o'clock, and
/// [`EXHAUSTED`](crate::sim::brain::routines::EXHAUSTED) — 75, where a nap
/// ties with work and wins — is six and a quarter points past that. Eight
/// hours at 1.1 is five more, so an office worker ends the day at 73.75: worn
/// out, in bed an hour before somebody with no job, but at the desk until five
/// rather than walking out of it for a nap at four.
pub const WORK_TIRES_LIKE: f32 = 1.1;

/// Stamina a world second of work costs **on top of** being awake for it —
/// the drain above runs through a shift as it does through any other hour.
pub const WORK_FATIGUE_PER_SECOND: f32 = ENERGY_PER_SECOND * (WORK_TIRES_LIKE - 1.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Energy;

impl Process for Energy {
    fn id(&self) -> ProcessId {
        ProcessId::Energy
    }

    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        stats.change_stamina(-ENERGY_PER_SECOND * dt);
    }

    fn handle(&mut self, event: Event, _novelty: f32, stats: &mut Stats) {
        match event {
            Event::Slept { world_seconds } => stats.change_stamina(RECOVERY_PER_SECOND * world_seconds),
            Event::Worked { world_seconds } => stats.change_stamina(-WORK_FATIGUE_PER_SECOND * world_seconds),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::biology::recollection::FRESH;
    use crate::sim::clock::{MINUTE, TIME_SCALE};
    use crate::sim::item::ItemKind;

    /// One tick, in world seconds — the same rig `bladder.rs` uses.
    const DT: f32 = TIME_SCALE / 64.0;

    #[test]
    fn time_alone_tires_a_person_and_never_past_exhausted() {
        let mut stats = Stats::calm().with_stamina(90.0);
        Energy.advance(&mut stats, 5.0 * MINUTE);
        assert_eq!(stats.stamina(), 90.0 - 5.0 * MINUTE * ENERGY_PER_SECOND);
        Energy.advance(&mut stats, 40.0 * HOUR);
        assert_eq!(stats.stamina(), 0.0);
        assert_eq!(stats.tiredness(), 100.0);
    }

    #[test]
    fn a_waking_day_is_what_it_takes_to_be_exhausted() {
        let mut stats = Stats::calm().with_stamina(100.0);
        Energy.advance(&mut stats, 15.0 * HOUR);
        assert!(stats.tiredness() < 100.0, "{}", stats.tiredness());
        Energy.advance(&mut stats, 1.5 * HOUR);
        assert_eq!(stats.tiredness(), 100.0);
    }

    /// The point of adding the drain back: sleep is judged by what is left
    /// once the body has also spent the night getting tired.
    #[test]
    fn eight_hours_in_bed_undo_a_whole_day_even_though_the_body_keeps_tiring() {
        let mut stats = Stats::calm().with_stamina(0.0);
        for hour in 0..HOURS_OF_SLEEP as u32 {
            // An hour a chunk, as the goal does it: the drain runs through the
            // hour, and the rest arrives when the hour is over.
            for _ in 0..(HOUR / DT) as u32 {
                Energy.advance(&mut stats, DT);
            }
            Energy.handle(Event::Slept { world_seconds: HOUR }, FRESH, &mut stats);
            assert!(stats.stamina() > 0.0, "hour {hour}");
        }
        assert!((stats.stamina() - 100.0).abs() < 1.0, "{}", stats.stamina());
    }

    #[test]
    fn rest_never_goes_past_fully_rested_however_long_somebody_sleeps() {
        let mut stats = Stats::calm().with_stamina(80.0);
        Energy.handle(Event::Slept { world_seconds: 20.0 * HOUR }, FRESH, &mut stats);
        assert_eq!(stats.stamina(), 100.0);
    }

    #[test]
    fn a_short_sleep_is_worth_a_short_rest() {
        let mut stats = Stats::calm().with_stamina(20.0);
        Energy.handle(Event::Slept { world_seconds: HOUR }, FRESH, &mut stats);
        assert!(stats.stamina() > 20.0 && stats.stamina() < 50.0, "{}", stats.stamina());
    }

    #[test]
    fn an_hour_at_work_tires_more_than_an_hour_awake() {
        let (mut idle, mut working) = (Stats::calm().with_stamina(80.0), Stats::calm().with_stamina(80.0));
        Energy.advance(&mut idle, HOUR);
        Energy.advance(&mut working, HOUR);
        Energy.handle(Event::Worked { world_seconds: HOUR }, FRESH, &mut working);
        let (spent_idle, spent_working) = (80.0 - idle.stamina(), 80.0 - working.stamina());
        assert!(spent_working > spent_idle, "{spent_working} at work, {spent_idle} idle");
        assert!((spent_working / spent_idle - WORK_TIRES_LIKE).abs() < 1e-3);
    }

    /// The tuning `WORK_TIRES_LIKE` is chosen for: up rested at six, a shift
    /// from nine to five, and still short of exhausted at five o'clock —
    /// otherwise a nap (which wins a tie with work) would walk them out of
    /// the office every afternoon.
    #[test]
    fn a_rested_worker_is_worn_out_but_not_exhausted_at_the_end_of_a_shift() {
        use crate::sim::brain::routines::EXHAUSTED;
        let mut stats = Stats::calm().with_stamina(100.0);
        // Six to nine, then eight hours at the desk.
        Energy.advance(&mut stats, 3.0 * HOUR);
        for _ in 0..8 {
            Energy.advance(&mut stats, HOUR);
            Energy.handle(Event::Worked { world_seconds: HOUR }, FRESH, &mut stats);
        }
        assert!(stats.tiredness() < EXHAUSTED, "{} at five o'clock", stats.tiredness());
        assert!(stats.tiredness() > 11.0 * HOUR * ENERGY_PER_SECOND, "and more tired than a day off");
    }

    #[test]
    fn eating_the_toilet_and_a_computer_are_not_rest() {
        let mut stats = Stats::calm().with_stamina(30.0);
        Energy.handle(Event::Ingested(ItemKind::Food), FRESH, &mut stats);
        Energy.handle(Event::Relieved, FRESH, &mut stats);
        Energy.handle(Event::Entertained, FRESH, &mut stats);
        assert_eq!(stats.stamina(), 30.0);
    }
}
