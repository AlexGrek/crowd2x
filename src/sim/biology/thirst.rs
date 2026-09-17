//! [`Thirst`]: thirstier with time, less thirsty for drinking.

use super::{Event, Process, ProcessId, Stats};

/// Thirst gained per second: half as fast again as hunger, so quenched to
/// [`THIRSTY`] takes about forty seconds and a person drinks more often than
/// they eat.
///
/// [`THIRSTY`]: crate::sim::brain::routines::THIRSTY
pub const THIRST_PER_SECOND: f32 = 1.5;

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
    use crate::sim::item::{ItemKind, DRINK};

    #[test]
    fn time_makes_a_person_thirstier_faster_than_hungrier_and_never_past_parched() {
        let mut stats = Stats::calm().with_thirst(10.0);
        Thirst.advance(&mut stats, 5.0);
        assert_eq!(stats.thirst(), 10.0 + 5.0 * THIRST_PER_SECOND);
        assert!(THIRST_PER_SECOND > HUNGER_PER_SECOND);
        Thirst.advance(&mut stats, 10_000.0);
        assert_eq!(stats.thirst(), 100.0);
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
