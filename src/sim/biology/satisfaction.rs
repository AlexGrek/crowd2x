//! [`Satisfaction`]: content with life, worn down by time, lifted by a treat.
//!
//! The second stat that falls on its own, like [`Fun`](super::Fun), and the
//! first that nothing in particular *fixes*: there is no prop for it. It goes
//! up when something good happens — a tasty meal, a go on the computer — by as
//! much as that was worth **this time**, which is where
//! [`Recollection`](super::Recollection) comes in: the third meal of the same
//! food in a day is as filling as the first and much less of a treat.
//!
//! Seeing somebody lifts it a little too ([`GLAD_TO_SEE`]): every face that
//! comes into view and was not in mind, which is the brain's attention's to
//! say (`brain::attention`), not this process's — by less for every other
//! face met lately, since company is in recent memory like any treat.
//!
//! Nothing reads it to decide anything yet. It is a readout of how a person's
//! day is going, for the day a routine wants to know.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// World hours from thoroughly content to thoroughly discontented, if nothing
/// good happens at all.
///
/// Two days: a slow stat, since what lifts it comes a few times a day and only
/// a dull day after a dull day should leave somebody miserable.
pub const HOURS_TO_DISCONTENT: f32 = 48.0;

/// Satisfaction lost per **world** second — [`Think::game_dt`], never `dt`.
///
/// [`Think::game_dt`]: crate::sim::entity::Think::game_dt
pub const SATISFACTION_PER_SECOND: f32 = 100.0 / (HOURS_TO_DISCONTENT * HOUR);

/// How much satisfaction a go at something entertaining is worth, fresh.
///
/// Rather more than a meal's [`taste`](crate::sim::item::ItemKind::taste):
/// it is the thing done for its own sake, and it comes once a day where meals
/// come three or four times.
pub const ENJOYMENT: f32 = 20.0;

/// How much satisfaction seeing somebody is worth — somebody who was not in
/// mind, near or far.
///
/// A third of a meal's [`taste`](crate::sim::item::ItemKind::taste) for
/// somebody met when nobody has been lately, and less for each meeting since
/// ([`Experience::Company`](super::Experience::Company) has no floor): a crowd
/// can lift a day by at most about `GLAD_TO_SEE * ln 2 / 4` ≈ 0.9 an hour,
/// under half of what a day wears off, so company slows discontent and never
/// pins anybody at content.
pub const GLAD_TO_SEE: f32 = 5.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Satisfaction;

impl Process for Satisfaction {
    fn id(&self) -> ProcessId {
        ProcessId::Satisfaction
    }

    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        stats.change_satisfaction(-SATISFACTION_PER_SECOND * dt);
    }

    /// Something good happened: its worth, scaled by how fresh it was.
    ///
    /// A meal is worth its item's `taste` — how filling it is is the stomach's
    /// business and is not dulled by anything — and a go on the computer is
    /// worth [`ENJOYMENT`].
    fn handle(&mut self, event: Event, novelty: f32, stats: &mut Stats) {
        match event {
            Event::Ingested(item) => stats.change_satisfaction(item.taste() * novelty),
            Event::Entertained => stats.change_satisfaction(ENJOYMENT * novelty),
            Event::Met => stats.change_satisfaction(GLAD_TO_SEE * novelty),
            Event::Relieved | Event::Slept { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::biology::recollection::FRESH;
    use crate::sim::clock::MINUTE;
    use crate::sim::item::{ItemKind, TASTY};

    #[test]
    fn time_wears_satisfaction_down_and_never_past_nothing() {
        let mut stats = Stats::calm().with_satisfaction(80.0);
        Satisfaction.advance(&mut stats, 30.0 * MINUTE);
        assert_eq!(stats.satisfaction(), 80.0 - 30.0 * MINUTE * SATISFACTION_PER_SECOND);
        Satisfaction.advance(&mut stats, 100.0 * HOUR);
        assert_eq!(stats.satisfaction(), 0.0);
    }

    /// Slow: a day with nothing good in it takes half of it away, not all.
    #[test]
    fn a_whole_day_of_nothing_good_leaves_a_content_person_only_half_content() {
        let mut stats = Stats::calm().with_satisfaction(100.0);
        Satisfaction.advance(&mut stats, 24.0 * HOUR);
        assert!((stats.satisfaction() - 50.0).abs() < 0.01, "{}", stats.satisfaction());
    }

    #[test]
    fn a_tasty_meal_and_a_go_on_the_computer_are_both_a_treat() {
        let mut stats = Stats::calm().with_satisfaction(30.0);
        Satisfaction.handle(Event::Ingested(ItemKind::Food), FRESH, &mut stats);
        assert_eq!(stats.satisfaction(), 30.0 + TASTY);
        Satisfaction.handle(Event::Entertained, FRESH, &mut stats);
        assert_eq!(stats.satisfaction(), 30.0 + TASTY + ENJOYMENT);
    }

    #[test]
    fn something_had_recently_is_worth_only_its_novelty() {
        let mut stats = Stats::calm().with_satisfaction(30.0);
        Satisfaction.handle(Event::Ingested(ItemKind::Food), 0.5, &mut stats);
        assert_eq!(stats.satisfaction(), 30.0 + TASTY * 0.5);
        Satisfaction.handle(Event::Entertained, 0.25, &mut stats);
        assert_eq!(stats.satisfaction(), 30.0 + TASTY * 0.5 + ENJOYMENT * 0.25);
    }

    #[test]
    fn seeing_somebody_is_a_little_lift_and_less_of_one_in_company() {
        let mut stats = Stats::calm().with_satisfaction(30.0);
        Satisfaction.handle(Event::Met, FRESH, &mut stats);
        assert_eq!(stats.satisfaction(), 30.0 + GLAD_TO_SEE);
        Satisfaction.handle(Event::Met, 0.25, &mut stats);
        assert_eq!(stats.satisfaction(), 30.0 + 1.25 * GLAD_TO_SEE);
        assert!(GLAD_TO_SEE < crate::sim::item::TASTY, "and less than a meal");
    }

    #[test]
    fn water_the_toilet_and_sleep_are_no_treat() {
        let mut stats = Stats::calm().with_satisfaction(30.0);
        Satisfaction.handle(Event::Ingested(ItemKind::Water), FRESH, &mut stats);
        Satisfaction.handle(Event::Relieved, FRESH, &mut stats);
        Satisfaction.handle(Event::Slept { world_seconds: HOUR }, FRESH, &mut stats);
        assert_eq!(stats.satisfaction(), 30.0);
    }
}
