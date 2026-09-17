//! [`Thirst`]: thirstier with time, less thirsty for drinking.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// World hours from quenched to parched: well under what hunger takes, so a
/// person is [`THIRSTY`] three hours after a drink and drinks half as often
/// again as they eat.
///
/// [`THIRSTY`]: crate::sim::brain::routines::THIRSTY
pub const HOURS_TO_PARCHED: f32 = 5.0;

/// Thirst gained per **world** second — [`Think::game_dt`], never `dt`.
///
/// [`Think::game_dt`]: crate::sim::entity::Think::game_dt
pub const THIRST_PER_SECOND: f32 = 100.0 / (HOURS_TO_PARCHED * HOUR);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Thirst;

impl Process for Thirst {
    fn id(&self) -> ProcessId {
        ProcessId::Thirst
    }

    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        stats.change_thirst(THIRST_PER_SECOND * dt);
    }

    /// Whatever was drunk takes away as much thirst as it has water in it.
    fn handle(&mut self, event: Event, stats: &mut Stats) {
        if let Event::Ingested(item) = event {
            stats.change_thirst(-item.hydration());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::biology::hunger::HUNGER_PER_SECOND;
    use crate::sim::brain::routines::THIRSTY;
    use crate::sim::clock::MINUTE;
    use crate::sim::item::{ItemKind, DRINK};

    #[test]
    fn time_makes_a_person_thirstier_faster_than_hungrier_and_never_past_parched() {
        let mut stats = Stats::calm().with_thirst(10.0);
        Thirst.advance(&mut stats, 5.0 * MINUTE);
        assert_eq!(stats.thirst(), 10.0 + 5.0 * MINUTE * THIRST_PER_SECOND);
        assert!(THIRST_PER_SECOND > HUNGER_PER_SECOND);
        Thirst.advance(&mut stats, 10.0 * HOUR);
        assert_eq!(stats.thirst(), 100.0);
    }

    /// Hours, not seconds — and fewer of them than a meal is apart.
    #[test]
    fn a_dry_mouth_takes_hours_to_become_thirsty() {
        let mut stats = Stats::calm().with_thirst(0.0);
        Thirst.advance(&mut stats, 2.5 * HOUR);
        assert!(stats.thirst() < THIRSTY, "{}", stats.thirst());
        Thirst.advance(&mut stats, 1.0 * HOUR);
        assert!(stats.thirst() > THIRSTY, "{}", stats.thirst());
    }

    #[test]
    fn drinking_takes_thirst_away_and_never_past_quenched() {
        let mut stats = Stats::calm().with_thirst(70.0);
        Thirst.handle(Event::Ingested(ItemKind::Water), &mut stats);
        assert_eq!(stats.thirst(), 70.0 - DRINK);
        Thirst.handle(Event::Ingested(ItemKind::Water), &mut stats);
        assert_eq!(stats.thirst(), 0.0);
    }

    #[test]
    fn food_does_not_quench_thirst() {
        let mut stats = Stats::calm().with_thirst(70.0);
        Thirst.handle(Event::Ingested(ItemKind::Food), &mut stats);
        assert_eq!(stats.thirst(), 70.0);
    }
}
