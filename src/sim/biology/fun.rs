//! [`Fun`]: bored with time, entertained by something worth doing.
//!
//! The one process whose stat *falls* on its own. Hunger, thirst and a
//! bladder all rise towards the thing that has to be done about them; fun
//! drains away instead, and what the routine watching it reads is
//! [`Stats::boredom`] — the same number the other way up, so every need is
//! still on one scale.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// World hours from having a great time to thoroughly bored.
///
/// Twice what an empty stomach takes: being at a loose end is slower to
/// become pressing than being hungry is, so a person who has nothing else on
/// looks for something to do about once a day and drops it the moment a meal
/// matters more.
pub const HOURS_TO_BORED: f32 = 10.0;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fun;

impl Process for Fun {
    fn id(&self) -> ProcessId {
        ProcessId::Fun
    }

    /// Fun drains. Nothing is entertaining for ever, and a body left alone
    /// gets bored on its own — which is the whole of what this process does
    /// by itself.
    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        stats.change_fun(-FUN_PER_SECOND * dt);
    }

    /// Having done something entertaining puts [`AMUSEMENT`] back.
    ///
    /// What that something *was* is not this process's business, the same way
    /// a bladder does not care which toilet was used: a task says what
    /// happened to the body and the body decides what it comes to.
    fn handle(&mut self, event: Event, stats: &mut Stats) {
        if event == Event::Entertained {
            stats.change_fun(AMUSEMENT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        Fun.advance(&mut stats, 3.0 * HOUR);
        assert!(stats.boredom() < BORED, "{}", stats.boredom());
        Fun.advance(&mut stats, 3.5 * HOUR);
        assert!(stats.boredom() > BORED, "{}", stats.boredom());
    }

    #[test]
    fn a_go_at_something_entertaining_is_worth_more_than_the_wait_for_one() {
        let mut stats = Stats::calm().with_fun(100.0 - BORED);
        assert!(stats.boredom() >= BORED, "bored enough to want a go");
        Fun.handle(Event::Entertained, &mut stats);
        assert!(stats.boredom() <= AMUSED, "{} left", stats.boredom());
    }

    #[test]
    fn fun_never_goes_past_having_a_great_time_however_many_goes_it_takes() {
        let mut stats = Stats::calm().with_fun(50.0);
        for _ in 0..10 {
            Fun.handle(Event::Entertained, &mut stats);
        }
        assert_eq!(stats.fun(), 100.0);
    }

    #[test]
    fn eating_and_the_toilet_are_not_entertainment() {
        let mut stats = Stats::calm().with_fun(30.0);
        Fun.handle(Event::Ingested(ItemKind::Food), &mut stats);
        Fun.handle(Event::Relieved, &mut stats);
        assert_eq!(stats.fun(), 30.0);
    }
}
