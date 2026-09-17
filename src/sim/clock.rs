//! How long a second is: [`TIME_SCALE`], and the [`Clock`] that follows from
//! it.
//!
//! **A second of watching is two minutes of world.** That is the one number
//! this module exists for, and everything about time in the simulation is
//! either measured in world seconds or converted through it.
//!
//! # Two clocks, and which is which
//!
//! A tick is handed `dt` — the slice of *watched* time it covers, the fixed
//! timestep. Two different things are then measured with it:
//!
//! * **What is watched happening** runs on that second. A walk crosses cells
//!   at a pace a person can follow, a meal has a progress bar that fills while
//!   somebody stands there, a unit stands aside for half a second to let
//!   another past. Scaling these by [`TIME_SCALE`] would make a crowd
//!   teleport: at 64Hz a tick is two minutes of world, and a body that walked
//!   two minutes' worth of cells in one tick would cross a room between two
//!   frames.
//! * **What the world's clock governs** runs a hundred and twenty times as
//!   fast. Getting hungry takes hours, not seconds, and hours are what the
//!   clock counts — so a body ages by [`Think::game_dt`] and never by `dt`.
//!
//! [`Think::game_dt`]: crate::sim::entity::Think::game_dt
//!
//! So a duration in this simulation is one of two things, and the unit it is
//! written in says which:
//!
//! ```text
//! HUNGER_PER_SECOND = 100.0 / (6.0 * HOUR)   world seconds: six hours to starving
//! CHEW_SECONDS      = watched(15.0 * MINUTE) watched seconds: a quarter hour of eating,
//!                                            seven and a half seconds of watching it
//! ```
//!
//! [`watched`] is how the second kind is written, so that even a duration the
//! mechanics measure on the wall clock says what the world thinks is going on.
//!
//! # What it does not touch
//!
//! The game speed (`game::speed`) multiplies how many *ticks* happen, so it
//! moves both clocks together and by the same factor — 4x is four times as
//! much world watched in the same time, not a different world.

use std::fmt;

/// World seconds in one watched second: **one second is two minutes**.
///
/// Changing this changes the pace of everything the clock governs at once —
/// needs, the time of day, and every duration written through [`watched`] —
/// because none of them hold a converted number of their own.
pub const TIME_SCALE: f32 = 120.0;

/// A world minute, in world seconds. Here rather than assumed, so a rate
/// written as `100.0 / (6.0 * HOUR)` says what it means at the point it is
/// defined.
pub const MINUTE: f32 = 60.0;

/// A world hour, in world seconds. Thirty seconds of watching.
pub const HOUR: f32 = 60.0 * MINUTE;

/// A world day, in world seconds. Twelve minutes of watching.
pub const DAY: f32 = 24.0 * HOUR;

/// What the clock says when a world is created: eight in the morning, so a day
/// starts where a person's does rather than in the middle of the night.
pub const OPENING_TIME: f64 = 8.0 * HOUR as f64;

/// How long `world_seconds` of world takes to watch, in watched seconds.
///
/// For the durations that are *measured* on the watched clock because they are
/// watched — an action a body is part way through, a pause to let somebody
/// past — but that mean something in world time. Write the world meaning and
/// let this convert it: `watched(15.0 * MINUTE)` is a quarter of an hour of
/// eating, and nobody has to work out that 7.5 was ever a quarter of an hour.
pub const fn watched(world_seconds: f32) -> f32 {
    world_seconds / TIME_SCALE
}

/// What time it is in the world.
///
/// Derived from how long the world has been watched rather than accumulated
/// beside it — there is one clock in a `GameState` and this is a reading of
/// it, so the two can never drift apart.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub struct Clock {
    /// World seconds since midnight of day one, [`OPENING_TIME`] included.
    since_midnight: f64,
}

impl Clock {
    /// The clock of a world that has been watched for `seconds`.
    pub fn after_watching(seconds: f64) -> Clock {
        Clock {
            since_midnight: OPENING_TIME + seconds * TIME_SCALE as f64,
        }
    }

    /// World seconds since the world opened. What a duration in world time is
    /// measured against; the time of day below is what it is *displayed* as.
    pub fn elapsed(&self) -> f64 {
        self.since_midnight - OPENING_TIME
    }

    /// Which day it is, counting from one. A day is twelve minutes of
    /// watching, so this moves often enough to be worth showing.
    pub fn day(&self) -> u32 {
        (self.since_midnight / DAY as f64) as u32 + 1
    }

    /// The hour of the day, 0 to 23.
    pub fn hour(&self) -> u32 {
        (self.seconds_today() / HOUR as f64) as u32
    }

    /// The minute of the hour, 0 to 59. Two of these a second.
    pub fn minute(&self) -> u32 {
        (self.seconds_today() / MINUTE as f64) as u32 % 60
    }

    /// The second of the minute, 0 to 59. A hundred and twenty of these a
    /// second, so this is a blur — which is what the time scale looks like,
    /// and the reason it is shown rather than rounded away.
    pub fn second(&self) -> u32 {
        self.seconds_today() as u32 % 60
    }

    /// The time of day, `08:03:20`. Zero-padded and always the same width, so
    /// a readout that is rewritten every frame does not jitter.
    pub fn time(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour(), self.minute(), self.second())
    }

    fn seconds_today(&self) -> f64 {
        self.since_midnight.rem_euclid(DAY as f64)
    }
}

/// `day 1  08:03:20` — what the HUD shows.
impl fmt::Display for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "day {}  {}", self.day(), self.time())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_of_watching_is_two_minutes_of_world() {
        assert_eq!(TIME_SCALE, 2.0 * MINUTE);
        let clock = Clock::after_watching(1.0);
        assert_eq!(clock.elapsed(), 2.0 * MINUTE as f64);
        assert_eq!(clock.time(), "08:02:00");
    }

    #[test]
    fn a_world_opens_at_eight_in_the_morning_on_day_one() {
        let clock = Clock::after_watching(0.0);
        assert_eq!(clock.time(), "08:00:00");
        assert_eq!(clock.day(), 1);
        assert_eq!(clock.elapsed(), 0.0);
    }

    #[test]
    fn watching_for_half_a_minute_is_an_hour_of_world() {
        let clock = Clock::after_watching(30.0);
        assert_eq!(clock.hour(), 9);
        assert_eq!(clock.minute(), 0);
        assert_eq!(clock.time(), "09:00:00");
    }

    /// Midnight is sixteen hours after opening: eight watched minutes.
    #[test]
    fn the_day_turns_over_at_midnight_and_the_time_starts_again() {
        let midnight = Clock::after_watching((16.0 * HOUR / TIME_SCALE) as f64);
        assert_eq!(midnight.time(), "00:00:00");
        assert_eq!(midnight.day(), 2);

        let later = Clock::after_watching(((16.0 * HOUR + 90.0 * MINUTE) / TIME_SCALE) as f64);
        assert_eq!(later.time(), "01:30:00");
        assert_eq!(later.day(), 2);
    }

    #[test]
    fn a_whole_day_is_twelve_minutes_of_watching() {
        let clock = Clock::after_watching(12.0 * 60.0);
        assert_eq!(clock.day(), 2);
        assert_eq!(clock.time(), "08:00:00");
        assert_eq!(clock.elapsed(), DAY as f64);
    }

    #[test]
    fn the_readout_is_always_the_same_width() {
        for seconds in [0.0, 0.5, 7.0, 61.0, 800.0, 12345.0] {
            assert_eq!(Clock::after_watching(seconds).time().len(), 8, "{seconds}");
        }
    }

    /// The conversion both ways round, since every duration in the brain is
    /// written through it.
    #[test]
    fn a_duration_written_in_world_units_is_watched_for_a_hundred_and_twentieth_of_it() {
        assert_eq!(watched(2.0 * MINUTE), 1.0);
        assert_eq!(watched(15.0 * MINUTE), 7.5);
        assert_eq!(watched(HOUR), 30.0);
    }
}
