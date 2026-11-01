//! The last week of the world, an hour at a time: how the crowd feels and
//! how it is off, for the statistics menu's graphs.
//!
//! [`History`] keeps one [`Sample`] for every world hour, the newest
//! [`HOURS`] of them (seven days): the mean mental health and the mean health
//! of the living humans, the district's budget and the cash all the living
//! hold between them. A sample is taken on the tick a world hour begins (and
//! on the first tick of a world with none), by one walk of the arena — an
//! hour of world time is thousands of ticks, so it costs nothing that shows.
//! The buffer is sized once, so taking a sample in the processing pass
//! allocates nothing. Nothing reads it but a screen; it is saved so a loaded
//! game draws the week it had.

use std::collections::VecDeque;

use crate::save::{invalid, pb, Invalid};

use super::clock::{self, Clock};
use super::economy::{Economy, Money};
use super::entities::Entities;
use super::uid::EntityType;

/// How many samples are kept: an hour each, seven days.
pub const HOURS: usize = 7 * 24;

/// The world at the start of one hour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Whole world hours since the world began.
    pub hour: u64,
    /// The living humans the means are of.
    pub humans: u32,
    /// Mean mental health of the living humans, 0-100; 0 with none.
    pub mind: f32,
    /// Mean health of the living humans, 0-100; 0 with none.
    pub health: f32,
    /// The district's budget.
    pub budget: Money,
    /// The cash every living account holds, debts taken off.
    pub cash: Money,
}

impl Sample {
    /// The world as it is at `now`.
    pub(crate) fn of(now: Clock, entities: &Entities, economy: &Economy) -> Sample {
        let (mut humans, mut mind, mut health) = (0u32, 0.0f64, 0.0f64);
        for entity in entities.iter() {
            if entity.is_dead() || entity.uid().kind() != Some(EntityType::Human) {
                continue;
            }
            let Some(biology) = entity.biology() else { continue };
            humans += 1;
            mind += biology.stats().mental_health() as f64;
            health += biology.stats().health() as f64;
        }
        let mean = |sum: f64| if humans == 0 { 0.0 } else { (sum / humans as f64) as f32 };
        let cash = economy.accounts().iter().filter(|account| !account.dead).map(|account| account.cash.as_cents()).sum();
        Sample { hour: hour_of(now), humans, mind: mean(mind), health: mean(health), budget: economy.budget(), cash: Money(cash) }
    }
}

/// Whole world hours since the world began.
pub fn hour_of(now: Clock) -> u64 {
    (now.elapsed() / clock::HOUR as f64).floor() as u64
}

/// The newest week of samples, oldest first.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    samples: VecDeque<Sample>,
}

impl Default for History {
    fn default() -> Self {
        History { samples: VecDeque::with_capacity(HOURS) }
    }
}

impl History {
    /// Whether a sample is due on the tick that went from `before` to `now`:
    /// a world hour began on it, or there is no sample yet.
    pub(crate) fn due(&self, before: Clock, now: Clock) -> bool {
        self.samples.is_empty() || hour_of(now) > hour_of(before)
    }

    /// Keep `sample`, letting the oldest go past [`HOURS`]. Never grows the
    /// buffer past the size it was made with.
    pub(crate) fn record(&mut self, sample: Sample) {
        if self.samples.len() == HOURS {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// The samples, oldest first.
    pub fn samples(&self) -> impl ExactSizeIterator<Item = &Sample> + DoubleEndedIterator + Clone {
        self.samples.iter()
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The newest sample, if there is one.
    pub fn latest(&self) -> Option<&Sample> {
        self.samples.back()
    }

    pub(crate) fn save(&self) -> Vec<pb::HourSample> {
        self.samples
            .iter()
            .map(|&Sample { hour, humans, mind, health, budget, cash }| pb::HourSample {
                hour,
                humans,
                mind,
                health,
                budget: budget.as_cents(),
                cash: cash.as_cents(),
            })
            .collect()
    }

    /// The samples as saved, the newest [`HOURS`] of them; none in a save
    /// from before there was a history.
    pub(crate) fn restore(saved: &[pb::HourSample]) -> Result<History, Invalid> {
        let mut history = History::default();
        let mut last = None;
        for sample in saved.iter().skip(saved.len().saturating_sub(HOURS)) {
            if last.is_some_and(|last| sample.hour <= last) {
                return invalid(format!("history: hour {} after hour {}", sample.hour, last.unwrap_or(0)));
            }
            if !(sample.mind.is_finite() && sample.health.is_finite()) {
                return invalid(format!("history: hour {} is not a number", sample.hour));
            }
            last = Some(sample.hour);
            history.record(Sample {
                hour: sample.hour,
                humans: sample.humans,
                mind: sample.mind,
                health: sample.health,
                budget: Money(sample.budget),
                cash: Money(sample.cash),
            });
        }
        Ok(history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(hour: u64) -> Sample {
        Sample { hour, humans: 2, mind: 50.0, health: 90.0, budget: Money(100), cash: Money(-5) }
    }

    #[test]
    fn a_week_is_kept_and_no_more() {
        let mut history = History::default();
        let capacity = history.samples.capacity();
        for hour in 0..HOURS as u64 + 10 {
            history.record(sample(hour));
        }
        assert_eq!(history.len(), HOURS);
        assert_eq!(history.samples().next().map(|sample| sample.hour), Some(10));
        assert_eq!(history.latest().map(|sample| sample.hour), Some(HOURS as u64 + 9));
        assert_eq!(history.samples.capacity(), capacity, "recording grew the buffer");
    }

    #[test]
    fn it_comes_back_as_it_was_saved() {
        let mut history = History::default();
        for hour in 3..40 {
            history.record(sample(hour));
        }
        assert_eq!(History::restore(&history.save()).ok(), Some(history));
        assert_eq!(History::restore(&[]).ok(), Some(History::default()));
    }

    #[test]
    fn hours_out_of_order_are_refused() {
        let saved = [5, 4].map(|hour| pb::HourSample { hour, ..Default::default() });
        assert!(History::restore(&saved).is_err());
    }
}
