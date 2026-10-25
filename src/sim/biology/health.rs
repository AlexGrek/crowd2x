//! [`Health`]: worn away by a need left at its worst, healed back while the
//! body is looked after.
//!
//! A need at the end of its range — starving, parched, exhausted — is not
//! only a priority any more: it hurts. Each one costs health at its own pace,
//! and together they cost the sum. While none of them is anywhere near its
//! end, health comes back. In between it holds still, so a unit that has just
//! eaten its way off the bottom does not get better before it is fed.
//!
//! Health at 0 is death. That is not this process's to decide — a process
//! only moves numbers — but the kind that carries the body reads it after
//! time has passed (`Human::react`), and [`cause_of_death`] says what did it.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// Hunger at which a body starves, and starving hurts: the top of its range.
pub const STARVING: f32 = 100.0;

/// Thirst at which a body is parched and dehydrating.
pub const PARCHED: f32 = 100.0;

/// Stamina at which a body is spent and collapsing.
pub const SPENT: f32 = 0.0;

/// World hours from full health to dead, starving and nothing else: a day.
/// On top of the eight it takes to get hungry that far.
pub const HOURS_TO_STARVE: f32 = 24.0;

/// World hours from full health to dead of thirst alone: half a day.
pub const HOURS_TO_DIE_OF_THIRST: f32 = 12.0;

/// World hours from full health to dead of exhaustion alone: two days.
pub const HOURS_TO_DIE_OF_EXHAUSTION: f32 = 48.0;

/// World hours from death's door back to full health, while well.
pub const HOURS_TO_HEAL: f32 = 24.0;

/// Below this much hunger and thirst, and above as much rest turned over,
/// a body is well and heals. The line is a long way from the end of each
/// range on purpose: a unit fed back from starving is not well yet.
pub const WELL: f32 = 80.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Health;

/// Health lost per **world** second to whatever is hurting the body now —
/// 0 when nothing is.
fn harm(stats: &Stats) -> f32 {
    let mut per_second = 0.0;
    if stats.hunger() >= STARVING {
        per_second += 100.0 / (HOURS_TO_STARVE * HOUR);
    }
    if stats.thirst() >= PARCHED {
        per_second += 100.0 / (HOURS_TO_DIE_OF_THIRST * HOUR);
    }
    if stats.stamina() <= SPENT {
        per_second += 100.0 / (HOURS_TO_DIE_OF_EXHAUSTION * HOUR);
    }
    per_second
}

fn is_well(stats: &Stats) -> bool {
    stats.hunger() < WELL && stats.thirst() < WELL && stats.tiredness() < WELL
}

/// What is killing a body: the first need at its end, in the order they
/// kill fastest. `"wounds"` for a body at 0 that none of them is hurting —
/// a test, or a save made by hand.
pub fn cause_of_death(stats: &Stats) -> &'static str {
    if stats.thirst() >= PARCHED {
        "thirst"
    } else if stats.hunger() >= STARVING {
        "starvation"
    } else if stats.stamina() <= SPENT {
        "exhaustion"
    } else {
        "wounds"
    }
}

impl Health {
    pub(super) fn advance_hospital(stats: &mut Stats, dt: f32) {
        stats.change_health(-harm(stats) * dt);
    }
}

impl Process for Health {
    fn id(&self) -> ProcessId {
        ProcessId::Health
    }

    fn handle(&mut self, event: Event, _novelty: f32, stats: &mut Stats) {
        if let Event::Treated { hp } = event { stats.change_health(hp.max(0.0)); }
        if let Event::Bitten { damage } = event {
            stats.change_health(-(damage as f32));
        }
    }

    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        let harm = harm(stats);
        if harm > 0.0 {
            stats.change_health(-harm * dt);
        } else if is_well(stats) {
            stats.change_health(100.0 / (HOURS_TO_HEAL * HOUR) * dt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_looked_after_heals_and_never_past_full() {
        let mut stats = Stats::calm().with_health(10.0);
        Health.advance(&mut stats, HOUR);
        assert!(stats.health() > 10.0, "{}", stats.health());
        Health.advance(&mut stats, 2.0 * HOURS_TO_HEAL * HOUR);
        assert_eq!(stats.health(), 100.0);
    }

    #[test]
    fn starving_kills_in_a_day_from_full_health() {
        let mut stats = Stats::calm().with_health(100.0).with_hunger(100.0);
        Health.advance(&mut stats, 0.9 * HOURS_TO_STARVE * HOUR);
        assert!(stats.health() > 0.0, "{}", stats.health());
        Health.advance(&mut stats, 0.2 * HOURS_TO_STARVE * HOUR);
        assert_eq!(stats.health(), 0.0);
        assert_eq!(cause_of_death(&stats), "starvation");
    }

    #[test]
    fn thirst_kills_faster_than_hunger_and_both_together_faster_still() {
        let lost = |stats: Stats| {
            let mut stats = stats.with_health(100.0);
            Health.advance(&mut stats, HOUR);
            100.0 - stats.health()
        };
        let starving = lost(Stats::calm().with_hunger(100.0));
        let parched = lost(Stats::calm().with_thirst(100.0));
        let both = lost(Stats::calm().with_hunger(100.0).with_thirst(100.0));
        assert!(parched > starving && starving > 0.0, "{parched} {starving}");
        assert!((both - (parched + starving)).abs() < 1e-3, "{both}");
    }

    #[test]
    fn exhaustion_hurts_too() {
        let mut stats = Stats::calm().with_health(50.0).with_stamina(0.0);
        Health.advance(&mut stats, HOUR);
        assert!(stats.health() < 50.0);
        assert_eq!(cause_of_death(&stats), "exhaustion");
    }

    /// Eaten off the bottom but still hungry: no worse, and no better yet.
    #[test]
    fn a_body_neither_hurting_nor_well_holds_still() {
        let mut stats = Stats::calm().with_health(40.0).with_hunger(90.0);
        Health.advance(&mut stats, HOUR);
        assert_eq!(stats.health(), 40.0);
    }
}
