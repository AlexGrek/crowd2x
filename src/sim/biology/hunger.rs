//! [`Hunger`]: hungrier with time, less hungry for eating.

use super::{Event, Process, ProcessId, Stats};

/// Hunger gained per second. From fed to [`PECKISH`] in about a minute, which is
/// slow enough that a crowd is not always eating and fast enough that watching
/// one person for a minute shows a meal.
///
/// [`PECKISH`]: crate::sim::brain::routines::PECKISH
pub const HUNGER_PER_SECOND: f32 = 1.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Hunger;

impl Process for Hunger {
    fn id(&self) -> ProcessId {
        ProcessId::Hunger
    }

    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        stats.change_hunger(HUNGER_PER_SECOND * dt);
    }

    /// Whatever was eaten takes away as much hunger as it is nourishing.
    fn handle(&mut self, event: Event, stats: &mut Stats) {
        if let Event::Ingested(item) = event {
            stats.change_hunger(-item.nutrition());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::item::{ItemKind, MEAL};

    #[test]
    fn time_makes_a_person_hungrier_and_never_past_starving() {
        let mut stats = Stats::calm().with_hunger(10.0);
        Hunger.advance(&mut stats, 5.0);
        assert_eq!(stats.hunger(), 10.0 + 5.0 * HUNGER_PER_SECOND);
        Hunger.advance(&mut stats, 10_000.0);
        assert_eq!(stats.hunger(), 100.0);
    }

    #[test]
    fn eating_takes_hunger_away_and_never_past_full() {
        let mut stats = Stats::calm().with_hunger(70.0);
        Hunger.handle(Event::Ingested(ItemKind::Food), &mut stats);
        assert_eq!(stats.hunger(), 70.0 - MEAL);
        Hunger.handle(Event::Ingested(ItemKind::Food), &mut stats);
        assert_eq!(stats.hunger(), 0.0);
    }

    #[test]
    fn water_is_not_a_meal() {
        let mut stats = Stats::calm().with_hunger(70.0);
        Hunger.handle(Event::Ingested(ItemKind::Water), &mut stats);
        assert_eq!(stats.hunger(), 70.0);
    }
}
