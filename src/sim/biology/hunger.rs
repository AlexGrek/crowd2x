//! [`Hunger`]: hungrier with time, less hungry for eating.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// World hours from a full stomach to starving, if nothing is eaten. A human
/// is [`PECKISH`] not quite five hours after a meal, so a day holds three or
/// four of them — which is what a day holds.
///
/// [`PECKISH`]: crate::sim::brain::routines::PECKISH
pub const HOURS_TO_STARVING: f32 = 8.0;

/// Hunger gained per **world** second — [`Think::game_dt`], never `dt`.
///
/// An hour of world is thirty seconds of watching
/// ([`TIME_SCALE`](crate::sim::clock::TIME_SCALE)), so a crowd watched at 1x
/// goes to eat every couple of minutes and one at 8x every twenty seconds.
/// Meals are meant to be occasional; the speed control is for watching a day
/// go by.
///
/// [`Think::game_dt`]: crate::sim::entity::Think::game_dt
pub const HUNGER_PER_SECOND: f32 = 100.0 / (HOURS_TO_STARVING * HOUR);

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
    use crate::sim::brain::routines::PECKISH;
    use crate::sim::clock::MINUTE;
    use crate::sim::item::{ItemKind, MEAL};

    #[test]
    fn time_makes_a_person_hungrier_and_never_past_starving() {
        let mut stats = Stats::calm().with_hunger(10.0);
        Hunger.advance(&mut stats, 5.0 * MINUTE);
        assert_eq!(stats.hunger(), 10.0 + 5.0 * MINUTE * HUNGER_PER_SECOND);
        Hunger.advance(&mut stats, 10.0 * HOUR);
        assert_eq!(stats.hunger(), 100.0);
    }

    /// The number the rate is chosen for: an empty stomach reaches
    /// [`PECKISH`] in a plausible few hours rather than in a plausible few
    /// seconds.
    #[test]
    fn an_empty_stomach_takes_hours_to_become_peckish() {
        let mut stats = Stats::calm().with_hunger(0.0);
        Hunger.advance(&mut stats, 4.0 * HOUR);
        assert!(stats.hunger() < PECKISH, "{}", stats.hunger());
        Hunger.advance(&mut stats, 1.0 * HOUR);
        assert!(stats.hunger() > PECKISH, "{}", stats.hunger());
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
