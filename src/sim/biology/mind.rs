//! [`Mind`]: mental health, worn down by a hard life and mended by a good one.
//!
//! The one process that owns [`Stats::mental_health`] (0, broken, to 100,
//! well). Slow, like everything about the mind: days, not hours. What moves it:
//!
//! * **Fun.** Fun under [`JOYLESS`] wears it down over [`HOURS_TO_BREAK_DOWN`];
//!   fun at [`CHEERFUL`] or over mends it — a share of what it lacks an hour
//!   ([`MENDING_HOURS`]), quickly from low and slowly near whole. In between
//!   it holds. The mood of the moment ([`Fun`](super::Fun)) is what the mind
//!   lives on. Because mending slows as it nears whole, a pressure that goes
//!   on (a debt) settles a cheerful mind at a level, rather than being
//!   outrun by cheer until everybody is at 100.
//! * **Work.** A stretch of work done rested is purpose, a little mending
//!   ([`PURPOSE`] an hour); done past [`STRAINED`] tiredness it is strain,
//!   wearing the mind down three times as fast ([`STRAIN`] an hour).
//!   [`Event::Worked`], from every task that is a job's work.
//! * **Fright.** A scare ([`Event::Frightened`]: a deathdog seen outside, or
//!   somebody seen running from one) is [`FRIGHT`] off at once, and a bite
//!   ([`Event::Bitten`]) [`BITE_SHOCK`] for every point of health it took.
//! * **Money.** A debt weighs on it ([`Event::Worried`]): the deeper the
//!   debt, up to [`DESPAIRING_DEBT`], the faster it wears the mind down, to
//!   [`HOURS_TO_DESPAIR`] at worst.
//! * **The air.** A stench wears it down and flowers mend it
//!   ([`Event::Smelled`]), by how strong the scent is, over [`HOURS_OF_SCENT`]
//!   at the strongest — the weakest of the five, where it was once the only
//!   one, and a point a look (every half world minute) swung a crowd's mind
//!   from one end to the other in a day.
//!
//! Smell and money are told on the brain's low beat, with the world time the
//! beat covers, so each is a rate like the rest. Asleep, the mind holds
//! still: no fun, no work, no worries, no scent.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// Fun under this is **joyless**: the mind wears down for as long as it stays
/// there. Eighteen world hours with nothing entertaining, from having a great
/// time.
pub const JOYLESS: f32 = 10.0;

/// World hours of joylessness that wear the mind from whole to nothing. Two
/// days: slow, and slower than any need.
pub const HOURS_TO_BREAK_DOWN: f32 = 48.0;

/// Fun at this or over is **cheerful**: the mind mends while it stays there.
/// Half way: somebody who has had a go at something today, or a chat.
pub const CHEERFUL: f32 = 50.0;

/// Cheerful, the mind mends what it lacks at a share of it an hour: the
/// lack over this many world hours (from 50, a point an hour; from 90, a
/// fifth of one). The same as [`HOURS_TO_DESPAIR`], so a cheerful mind under
/// a debt settles at 100 less the debt's share of [`DESPAIRING_DEBT`]: $50
/// owed, 80; $125, 50; $250, nothing.
pub const MENDING_HOURS: f32 = HOURS_TO_DESPAIR;

/// Mental health a world hour of work done rested is worth: purpose. A day's
/// shift, eight.
pub const PURPOSE: f32 = 1.0;

/// Mental health a world hour of work done past [`STRAINED`] costs: strain.
pub const STRAIN: f32 = 3.0;

/// Tiredness past which work is strain rather than purpose: exhausted, where
/// a bed by day is wanted (`brain::routines::EXHAUSTED`).
pub const STRAINED: f32 = 75.0;

/// Mental health a scare takes at once: a deathdog seen, somebody seen running
/// from one.
pub const FRIGHT: f32 = 8.0;

/// Mental health a bite takes for every point of health it took: six to
/// eighteen for a deathdog's 20 to 60.
pub const BITE_SHOCK: f32 = 0.3;

/// The debt, in dollars, at which money weighs on the mind as much as it ever
/// will: about a month of eating on credit on a care package.
pub const DESPAIRING_DEBT: f32 = 250.0;

/// World hours that a debt of [`DESPAIRING_DEBT`] or more wears the mind from
/// whole to nothing. A smaller debt, slower in proportion.
pub const HOURS_TO_DESPAIR: f32 = 48.0;

/// World hours that the strongest scent ([`smell::MAXIMUM`]) takes the mind
/// from one end to the other — down for a stench, up for flowers.
///
/// [`smell::MAXIMUM`]: crate::sim::smell::MAXIMUM
pub const HOURS_OF_SCENT: f32 = 72.0;

/// How much of its pace the mind keeps asleep: none.
pub const ASLEEP_PACE: f32 = 0.0;

/// A rate of `100` over `hours`, per world second.
const fn per_second(hours: f32) -> f32 {
    100.0 / (hours * HOUR)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mind;

impl Process for Mind {
    fn id(&self) -> ProcessId {
        ProcessId::Mind
    }

    fn asleep_pace(&self) -> f32 {
        ASLEEP_PACE
    }

    /// Joylessness wears it down, cheer mends it, and fun in between leaves it
    /// be.
    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        let fun = stats.fun();
        if fun < JOYLESS {
            stats.change_mental_health(-per_second(HOURS_TO_BREAK_DOWN) * dt);
        } else if fun >= CHEERFUL {
            let lack = 100.0 - stats.mental_health();
            stats.change_mental_health(lack / (MENDING_HOURS * HOUR) * dt);
        }
    }

    /// Work, frights, bites, debt and the air — none of them dulled by
    /// having had them lately: the tenth scare of a night is no easier.
    fn handle(&mut self, event: Event, _novelty: f32, stats: &mut Stats) {
        match event {
            Event::Worked { world_seconds } if world_seconds.is_finite() => {
                let hours = world_seconds / HOUR;
                let change = if stats.tiredness() > STRAINED { -STRAIN } else { PURPOSE };
                stats.change_mental_health(change * hours);
            }
            Event::Frightened => stats.change_mental_health(-FRIGHT),
            Event::Bitten { damage } => stats.change_mental_health(-BITE_SHOCK * damage as f32),
            Event::Worried { debt, world_seconds } if debt.is_finite() && world_seconds.is_finite() => {
                let weight = (debt / DESPAIRING_DEBT).clamp(0.0, 1.0);
                stats.change_mental_health(-weight * per_second(HOURS_TO_DESPAIR) * world_seconds);
            }
            Event::Smelled { value, world_seconds } if value.is_finite() && world_seconds.is_finite() => {
                let strength = (value / crate::sim::smell::MAXIMUM).clamp(-1.0, 1.0);
                stats.change_mental_health(strength * per_second(HOURS_OF_SCENT) * world_seconds);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::biology::recollection::FRESH;
    use crate::sim::clock::MINUTE;
    use crate::sim::item::ItemKind;

    fn mind(mental_health: f32) -> Stats {
        Stats::calm().with_mental_health(mental_health)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// Fun under ten wears the mind down at its rate, fun at fifty or over
    /// mends a share of what it lacks — more from low, less near whole — and
    /// fun in between leaves it be.
    #[test]
    fn joylessness_wears_the_mind_down_cheer_mends_what_it_lacks_and_in_between_holds() {
        let mut stats = mind(50.0).with_fun(JOYLESS - 1.0);
        Mind.advance(&mut stats, HOUR);
        assert!(close(stats.mental_health(), 50.0 - 100.0 / HOURS_TO_BREAK_DOWN), "{}", stats.mental_health());

        for from in [10.0, 50.0, 90.0] {
            let mut stats = mind(from).with_fun(CHEERFUL);
            Mind.advance(&mut stats, MINUTE);
            let mended = (stats.mental_health() - from) * 60.0;
            assert!((mended - (100.0 - from) / MENDING_HOURS).abs() < 1e-2, "from {from}: {mended} an hour");
        }

        let mut stats = mind(50.0).with_fun((JOYLESS + CHEERFUL) / 2.0);
        Mind.advance(&mut stats, 10.0 * HOUR);
        assert_eq!(stats.mental_health(), 50.0, "neither");

        let mut stats = mind(50.0).with_fun(0.0);
        Mind.advance(&mut stats, 10.0 * HOURS_TO_BREAK_DOWN * HOUR);
        assert_eq!(stats.mental_health(), 0.0, "never past nothing");
        let mut stats = mind(50.0).with_fun(100.0);
        for _ in 0..(10.0 * MENDING_HOURS) as u32 {
            Mind.advance(&mut stats, HOUR);
        }
        assert!((99.9..=100.0).contains(&stats.mental_health()), "near whole and never past: {}", stats.mental_health());
    }

    /// A shift done rested is purpose; done exhausted, strain.
    #[test]
    fn work_done_rested_is_purpose_and_work_done_exhausted_is_strain() {
        let shift = Event::Worked { world_seconds: 8.0 * HOUR };
        let mut rested = mind(50.0).with_stamina(100.0 - STRAINED);
        Mind.handle(shift, FRESH, &mut rested);
        assert!(close(rested.mental_health(), 50.0 + 8.0 * PURPOSE), "{}", rested.mental_health());

        let mut spent = mind(50.0).with_stamina(100.0 - STRAINED - 1.0);
        Mind.handle(shift, FRESH, &mut spent);
        assert!(close(spent.mental_health(), 50.0 - 8.0 * STRAIN), "{}", spent.mental_health());
    }

    #[test]
    fn a_fright_and_a_bite_each_take_their_share_at_once() {
        let mut stats = mind(50.0);
        Mind.handle(Event::Frightened, FRESH, &mut stats);
        assert_eq!(stats.mental_health(), 50.0 - FRIGHT);
        Mind.handle(Event::Bitten { damage: 40 }, FRESH, &mut stats);
        assert!(close(stats.mental_health(), 50.0 - FRIGHT - 40.0 * BITE_SHOCK), "{}", stats.mental_health());
    }

    /// The deeper the debt, the faster, up to a despairing debt; no debt, no
    /// worry.
    #[test]
    fn a_debt_weighs_on_the_mind_by_how_deep_it_is() {
        let worry = |debt: f32| {
            let mut stats = mind(50.0);
            Mind.handle(Event::Worried { debt, world_seconds: HOUR }, FRESH, &mut stats);
            50.0 - stats.mental_health()
        };
        let worst = 100.0 / HOURS_TO_DESPAIR;
        assert_eq!(worry(0.0), 0.0);
        assert!(close(worry(DESPAIRING_DEBT / 2.0), worst / 2.0), "{}", worry(DESPAIRING_DEBT / 2.0));
        assert!(close(worry(DESPAIRING_DEBT), worst));
        assert!(close(worry(10.0 * DESPAIRING_DEBT), worst), "no worse past despair");
    }

    /// Cheer and a debt that goes on settle the mind at 100 less the debt's
    /// share of a despairing one: a level, not a race to 100 or to nothing.
    #[test]
    fn a_cheerful_mind_under_a_debt_settles_at_a_level_set_by_the_debt() {
        for (debt, level) in [(50.0, 80.0), (125.0, 50.0), (250.0, 0.0)] {
            for from in [0.0, 100.0] {
                let mut stats = mind(from).with_fun(80.0);
                for _ in 0..(20.0 * MENDING_HOURS) as u32 {
                    Mind.advance(&mut stats, HOUR);
                    Mind.handle(Event::Worried { debt, world_seconds: HOUR }, FRESH, &mut stats);
                }
                assert!((stats.mental_health() - level).abs() < 1.5, "${debt} from {from}: {}", stats.mental_health());
            }
        }
    }

    /// A stench wears it down and flowers lift it, by strength — and a look
    /// at the air is worth the time it covers, not a point.
    #[test]
    fn the_air_moves_the_mind_by_how_strong_the_scent_is_and_how_long() {
        let breathe = |value: f32, world_seconds: f32| {
            let mut stats = mind(50.0);
            Mind.handle(Event::Smelled { value, world_seconds }, FRESH, &mut stats);
            stats.mental_health() - 50.0
        };
        let strongest = 100.0 / HOURS_OF_SCENT;
        assert!(close(breathe(-crate::sim::smell::MAXIMUM, HOUR), -strongest));
        assert!(close(breathe(crate::sim::smell::MAXIMUM / 2.0, HOUR), strongest / 2.0));
        assert_eq!(breathe(0.0, HOUR), 0.0);
        assert!(breathe(-crate::sim::smell::MAXIMUM, 0.5 * MINUTE).abs() < 0.02, "a look is a sliver, not a point");
    }

    #[test]
    fn eating_the_toilet_and_a_go_at_something_do_not_touch_the_mind_directly() {
        let mut stats = mind(50.0);
        for event in [Event::Ingested(ItemKind::Food), Event::Relieved, Event::Entertained, Event::Met, Event::Chatted] {
            Mind.handle(event, FRESH, &mut stats);
        }
        assert_eq!(stats.mental_health(), 50.0);
    }

    #[test]
    fn asleep_the_mind_holds_still() {
        assert_eq!(Mind.asleep_pace(), 0.0);
    }
}
