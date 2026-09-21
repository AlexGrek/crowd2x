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
//! it is made of and hunger deciding what that does.

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
/// The drain keeps running while somebody is asleep — a process does not know
/// what its body is doing — so it is added back here: the *net* gain is
/// `100 / HOURS_OF_SLEEP` an hour, and eight hours in bed is a full recovery.
pub const RECOVERY_PER_SECOND: f32 = 100.0 / (HOURS_OF_SLEEP * HOUR) + ENERGY_PER_SECOND;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Energy;

impl Process for Energy {
    fn id(&self) -> ProcessId {
        ProcessId::Energy
    }

    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        stats.change_stamina(-ENERGY_PER_SECOND * dt);
    }

    fn handle(&mut self, event: Event, stats: &mut Stats) {
        if let Event::Slept { world_seconds } = event {
            stats.change_stamina(RECOVERY_PER_SECOND * world_seconds);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            Energy.handle(Event::Slept { world_seconds: HOUR }, &mut stats);
            assert!(stats.stamina() > 0.0, "hour {hour}");
        }
        assert!((stats.stamina() - 100.0).abs() < 1.0, "{}", stats.stamina());
    }

    #[test]
    fn rest_never_goes_past_fully_rested_however_long_somebody_sleeps() {
        let mut stats = Stats::calm().with_stamina(80.0);
        Energy.handle(Event::Slept { world_seconds: 20.0 * HOUR }, &mut stats);
        assert_eq!(stats.stamina(), 100.0);
    }

    #[test]
    fn a_short_sleep_is_worth_a_short_rest() {
        let mut stats = Stats::calm().with_stamina(20.0);
        Energy.handle(Event::Slept { world_seconds: HOUR }, &mut stats);
        assert!(stats.stamina() > 20.0 && stats.stamina() < 50.0, "{}", stats.stamina());
    }

    #[test]
    fn eating_the_toilet_and_a_computer_are_not_rest() {
        let mut stats = Stats::calm().with_stamina(30.0);
        Energy.handle(Event::Ingested(ItemKind::Food), &mut stats);
        Energy.handle(Event::Relieved, &mut stats);
        Energy.handle(Event::Entertained, &mut stats);
        assert_eq!(stats.stamina(), 30.0);
    }
}
