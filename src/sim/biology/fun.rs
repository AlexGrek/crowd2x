//! [`Fun`]: bored with time, entertained by something worth doing.
//!
//! The one process whose stat *falls* on its own. Hunger, thirst and a
//! bladder all rise towards the thing that has to be done about them; fun
//! drains away instead, and what the routine watching it reads is
//! [`Stats::boredom`] — the same number the other way up, so every need is
//! still on one scale.
//!
//! Fun run too low ([`JOYLESS`]) wears down **mental health**
//! ([`Stats::mental_health`]): a body can go without entertainment for a
//! while at no cost, and not for ever.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// World hours from having a great time to thoroughly bored.
///
/// Twice what an empty stomach takes: being at a loose end is slower to
/// become pressing than being hungry is, so a person who has nothing else on
/// looks for something to do about once a day and drops it the moment a meal
/// matters more. (Twice what it was, like every need; a chat and a word in
/// passing are fun too, and keep it off longer.)
pub const HOURS_TO_BORED: f32 = 20.0;

/// Fun lost per **world** second — [`Think::game_dt`], never `dt`.
///
/// [`Think::game_dt`]: crate::sim::entity::Think::game_dt
pub const FUN_PER_SECOND: f32 = 100.0 / (HOURS_TO_BORED * HOUR);

/// How much fun one go at something entertaining is worth.
///
/// Enough to take somebody who only just got [`BORED`] well past
/// [`AMUSED`](crate::sim::brain::routines::AMUSED), the way a meal takes a
/// peckish person past [`SATED`](crate::sim::brain::routines::SATED): the
/// point of a session at the computer is that it is over with, not that it
/// takes the edge off and sends them straight back.
///
/// [`BORED`]: crate::sim::brain::routines::BORED
pub const AMUSEMENT: f32 = 60.0;

/// How much fun a chat is worth — a talk both sides agreed to, walked over for
/// and stood through to its end ([`Event::Chatted`]).
///
/// A third of a go at something entertaining: company is how most people keep
/// boredom off, and two hours of the drain it takes the edge off are about
/// right for a quarter of an hour of talk. Not dulled by recent memory — who
/// it is with is what makes a chat, and that is the relationship's business.
pub const CHAT_FUN: f32 = 20.0;

/// How much fun small talk is worth to each side ([`Event::SmallTalk`]): a
/// word in passing. A fraction of a chat's, so that it slows boredom and never
/// cures it.
pub const SMALL_TALK_FUN: f32 = 2.0;

/// Fun under this is **joyless**: mental health wears down for as long as it
/// stays there ([`MENTAL_HEALTH_PER_SECOND`]). Eighteen world hours with
/// nothing entertaining, from having a great time.
pub const JOYLESS: f32 = 10.0;

/// World hours of joylessness that wear mental health from whole to nothing.
/// Two days: slow, like everything about the mind, and slower than any need.
pub const HOURS_TO_BREAK_DOWN: f32 = 48.0;

/// Mental health lost per **world** second while joyless.
pub const MENTAL_HEALTH_PER_SECOND: f32 = 100.0 / (HOURS_TO_BREAK_DOWN * HOUR);

/// How much of its pace boredom keeps asleep: none. Nobody gets bored in their
/// sleep, and a sleeper who did was up at two in the morning for the computer.
pub const ASLEEP_PACE: f32 = 0.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fun;

impl Process for Fun {
    fn id(&self) -> ProcessId {
        ProcessId::Fun
    }

    fn asleep_pace(&self) -> f32 {
        ASLEEP_PACE
    }

    /// Fun drains. Nothing is entertaining for ever, and a body left alone
    /// gets bored on its own. And fun run under [`JOYLESS`] wears mental
    /// health down for every second it stays there — asleep, not at all,
    /// since asleep this whole process stops.
    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        stats.change_fun(-FUN_PER_SECOND * dt);
        if stats.fun() < JOYLESS {
            stats.change_mental_health(-MENTAL_HEALTH_PER_SECOND * dt);
        }
    }

    /// Having done something entertaining puts [`AMUSEMENT`] back — all of
    /// it the first time today, less for a go had again while the last one is
    /// still in recent memory (`novelty`, from
    /// [`Recollection`](super::Recollection)).
    ///
    /// What that something *was* is not this process's business, the same way
    /// a bladder does not care which toilet was used: a task says what
    /// happened to the body and the body decides what it comes to.
    fn handle(&mut self, event: Event, novelty: f32, stats: &mut Stats) {
        match event {
            Event::Smelled { value } if value.is_finite() => {
                let change = if value > 0.0 { 1.0 } else if value < 0.0 { -1.0 } else { 0.0 };
                stats.change_mental_health(change);
            }
            Event::Entertained => stats.change_fun(AMUSEMENT * novelty),
            // Talking to somebody is fun, and a chat more than a word.
            Event::Chatted => stats.change_fun(CHAT_FUN),
            Event::SmallTalk => stats.change_fun(SMALL_TALK_FUN),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::biology::recollection::FRESH;
    use crate::sim::brain::routines::{AMUSED, BORED};
    use crate::sim::clock::MINUTE;
    use crate::sim::item::ItemKind;

    #[test]
    fn time_alone_drains_the_fun_out_of_a_person_and_never_past_bored() {
        let mut stats = Stats::calm().with_fun(90.0);
        Fun.advance(&mut stats, 5.0 * MINUTE);
        assert_eq!(stats.fun(), 90.0 - 5.0 * MINUTE * FUN_PER_SECOND);
        Fun.advance(&mut stats, 20.0 * HOUR);
        assert_eq!(stats.fun(), 0.0);
        assert_eq!(stats.boredom(), 100.0);
    }

    /// Hours, and more of them than a meal is apart: being bored is the
    /// slowest of the needs to come round.
    #[test]
    fn a_person_with_nothing_to_do_takes_hours_to_get_bored() {
        let mut stats = Stats::calm().with_fun(100.0);
        Fun.advance(&mut stats, 11.0 * HOUR);
        assert!(stats.boredom() < BORED, "{}", stats.boredom());
        Fun.advance(&mut stats, 1.5 * HOUR);
        assert!(stats.boredom() > BORED, "{}", stats.boredom());
        assert!(HOURS_TO_BORED >= 2.0 * 10.0, "at least twice as slow as it was");
    }

    #[test]
    fn a_go_at_something_entertaining_is_worth_more_than_the_wait_for_one() {
        let mut stats = Stats::calm().with_fun(100.0 - BORED);
        assert!(stats.boredom() >= BORED, "bored enough to want a go");
        Fun.handle(Event::Entertained, FRESH, &mut stats);
        assert!(stats.boredom() <= AMUSED, "{} left", stats.boredom());
    }

    #[test]
    fn fun_never_goes_past_having_a_great_time_however_many_goes_it_takes() {
        let mut stats = Stats::calm().with_fun(50.0);
        for _ in 0..10 {
            Fun.handle(Event::Entertained, FRESH, &mut stats);
        }
        assert_eq!(stats.fun(), 100.0);
    }

    #[test]
    fn a_go_had_again_while_the_last_is_remembered_is_less_fun() {
        let mut stats = Stats::calm().with_fun(10.0);
        Fun.handle(Event::Entertained, 0.5, &mut stats);
        assert_eq!(stats.fun(), 10.0 + AMUSEMENT * 0.5);
    }

    #[test]
    fn eating_and_the_toilet_are_not_entertainment() {
        let mut stats = Stats::calm().with_fun(30.0);
        Fun.handle(Event::Ingested(ItemKind::Food), FRESH, &mut stats);
        Fun.handle(Event::Relieved, FRESH, &mut stats);
        assert_eq!(stats.fun(), 30.0);
    }

    /// Fun under ten wears the mind down at its rate, and not at ten or over.
    #[test]
    fn joylessness_wears_mental_health_down_and_fun_does_not() {
        let mut stats = Stats::calm().with_fun(JOYLESS - 1.0);
        let before = stats.mental_health();
        Fun.advance(&mut stats, HOUR);
        assert!((before - stats.mental_health() - HOUR * MENTAL_HEALTH_PER_SECOND).abs() < 1e-3, "{}", stats.mental_health());

        let mut stats = Stats::calm().with_fun(JOYLESS + 10.0);
        Fun.advance(&mut stats, HOUR);
        assert_eq!(stats.mental_health(), Stats::calm().mental_health(), "fun enough, no harm");

        let mut stats = Stats::calm().with_fun(0.0);
        Fun.advance(&mut stats, 10.0 * HOURS_TO_BREAK_DOWN * HOUR);
        assert_eq!(stats.mental_health(), 0.0, "never past nothing");
    }

    /// A chat is a good deal of fun, a word in passing a little, and neither
    /// is dulled by having had one lately.
    #[test]
    fn talking_to_somebody_is_fun() {
        let mut stats = Stats::calm().with_fun(30.0);
        Fun.handle(Event::Chatted, 0.25, &mut stats);
        assert_eq!(stats.fun(), 30.0 + CHAT_FUN);
        Fun.handle(Event::SmallTalk, 0.25, &mut stats);
        assert_eq!(stats.fun(), 30.0 + CHAT_FUN + SMALL_TALK_FUN);
        assert!(SMALL_TALK_FUN < CHAT_FUN && CHAT_FUN < AMUSEMENT);
    }

    /// The same computer, used for a living: no fun at all.
    #[test]
    fn a_shift_at_a_computer_is_no_fun() {
        let mut stats = Stats::calm().with_fun(30.0);
        Fun.handle(Event::Worked { world_seconds: 8.0 * HOUR }, FRESH, &mut stats);
        assert_eq!(stats.fun(), 30.0);
    }
}
