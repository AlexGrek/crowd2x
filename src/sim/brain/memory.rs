//! What a brain remembers: [`Memory`].

use std::collections::BTreeMap;

use crate::map::Point;
use crate::sim::clock::HOUR;
use crate::sim::uid::Uid;

/// The key a unit remembers its own bed under: a [`Recall::Cell`]. Written once,
/// when the unit arrives to find a bed free (`GameState::spawn`), and read by
/// the goal that puts it to bed.
pub const HOME_BED: &str = "home bed";

/// How long a place found out of reach is remembered as out of reach, in
/// **world** seconds: an hour of the world, half a minute of watching. Long
/// enough for the crowd round a toilet to clear, short enough that a doorway
/// somebody stood frozen in is tried again the same evening.
pub const OUT_OF_REACH_FOR: f64 = HOUR as f64;

/// How many places a brain can remember as out of reach at once. A fixed
/// array, so remembering allocates nothing in a tick; one more than this
/// pushes out the place that would have been forgiven soonest.
pub const OUT_OF_REACH_SLOTS: usize = 8;

/// What a brain remembers. **Nearly empty on purpose: the one keyed thing
/// written so far is which bed is its own** ([`HOME_BED`]); beside the keys,
/// the places it could not get to lately ([`OutOfReach`]).
///
/// A `BTreeMap` rather than a `HashMap`, and that is the only liberty taken
/// with the shape: iteration order has to be a declaration of the keys and not
/// an artefact of a hasher, or the day something iterates memory to make a
/// decision the thousand-tick determinism test starts failing once in every
/// few runs. Both allocate nothing while empty, which is today and for a while.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Memory {
    keyed: BTreeMap<String, Recall>,
    /// Boxed, out of line: allocated once with the brain in the spawn pass,
    /// and two hundred bytes a `Human` would otherwise carry inline against
    /// the size wall (`a_human_stays_small_enough_to_be_worth_a_thousand_of`).
    out_of_reach: Box<OutOfReach>,
}

/// Places — a toilet, a fridge, a bed — this unit tried to get to and could
/// not, each until a moment in world time when it is forgiven. What lets a
/// crowd jammed round one toilet door spread out to the others instead of
/// all waiting on the same one: each body that gives up on it remembers so,
/// and goes for the next nearest instead (`goals::nearest_in_reach`).
///
/// **It fades on its own**: an entry past its moment reads as never written,
/// with nothing to clean up, so a toilet that was jammed is tried again once
/// the world has moved on. Fixed-size and inline: remembering allocates
/// nothing in a tick.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct OutOfReach {
    /// `(place, forgiven at, in world seconds)`; `None` for an empty slot.
    places: [Option<(Point, f64)>; OUT_OF_REACH_SLOTS],
}

impl OutOfReach {
    /// Remember `place` as out of reach until [`OUT_OF_REACH_FOR`] after
    /// `now` (world seconds, `Clock::elapsed`). Remembering a place again
    /// extends it; a full memory loses the place that would be forgiven
    /// soonest, which is an expired one if there is any.
    pub fn remember(&mut self, place: Point, now: f64) {
        let until = now + OUT_OF_REACH_FOR;
        let slot = self
            .places
            .iter()
            .position(|entry| matches!(entry, Some((cell, _)) if *cell == place))
            .or_else(|| self.places.iter().position(Option::is_none))
            .unwrap_or_else(|| {
                // Every slot is full here. Ties go to the lower slot, so the
                // choice is the same on every run.
                let until_of = |i: usize| self.places[i].map_or(f64::NEG_INFINITY, |(_, until)| until);
                (0..OUT_OF_REACH_SLOTS).fold(0, |soonest, i| if until_of(i) < until_of(soonest) { i } else { soonest })
            });
        self.places[slot] = Some((place, until));
    }

    /// Whether `place` is still remembered as out of reach at `now`.
    pub fn contains(&self, place: Point, now: f64) -> bool {
        self.until(place, now).is_some()
    }

    /// When `place` is forgiven, in world seconds, if it is still remembered
    /// at `now`.
    pub fn until(&self, place: Point, now: f64) -> Option<f64> {
        self.places
            .iter()
            .flatten()
            .find(|&&(cell, until)| cell == place && now < until)
            .map(|&(_, until)| until)
    }

    /// Every place written, faded or not, with the world seconds it is
    /// forgiven at — for the brains menu, which has no clock to ask.
    pub fn all(&self) -> impl Iterator<Item = (Point, f64)> + '_ {
        self.places.iter().flatten().copied()
    }

    /// Every place still remembered at `now`, with the world seconds left on
    /// it, in slot order.
    pub fn iter(&self, now: f64) -> impl Iterator<Item = (Point, f64)> + '_ {
        self.places
            .iter()
            .flatten()
            .filter(move |&&(_, until)| now < until)
            .map(move |&(cell, until)| (cell, until - now))
    }
}

/// A reference to anything worth remembering. Deliberately few shapes — a brain
/// remembers places, people, numbers, moments and labels.
#[derive(Clone, PartialEq, Debug)]
pub enum Recall {
    Cell(Point),
    Entity(Uid),
    Number(f32),
    /// A tick.
    Time(u64),
    Text(String),
}

impl Recall {
    /// How it reads in a debug view.
    pub fn describe(&self) -> String {
        match self {
            Recall::Cell(cell) => format!("{}, {}", cell.x, cell.y),
            Recall::Entity(uid) => uid.to_string(),
            Recall::Number(number) => format!("{number:.2}"),
            Recall::Time(tick) => format!("tick {tick}"),
            Recall::Text(text) => text.clone(),
        }
    }
}

impl Memory {
    pub fn new() -> Memory {
        Memory::default()
    }

    pub fn get(&self, key: &str) -> Option<&Recall> {
        self.keyed.get(key)
    }

    /// Remember something under `key`, replacing whatever was there.
    pub fn remember(&mut self, key: impl Into<String>, recall: Recall) {
        self.keyed.insert(key.into(), recall);
    }

    /// Forget `key`; returns what it held.
    pub fn forget(&mut self, key: &str) -> Option<Recall> {
        self.keyed.remove(key)
    }

    /// How many keys it holds; places out of reach are not keys.
    pub fn len(&self) -> usize {
        self.keyed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keyed.is_empty()
    }

    /// In key order, which is the whole reason this is a `BTreeMap`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Recall)> {
        self.keyed.iter().map(|(key, recall)| (key.as_str(), recall))
    }

    /// The places it could not get to lately.
    pub fn out_of_reach(&self) -> &OutOfReach {
        &self.out_of_reach
    }

    pub fn out_of_reach_mut(&mut self) -> &mut OutOfReach {
        &mut self.out_of_reach
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_memory_is_empty() {
        assert!(Memory::new().is_empty());
        assert_eq!(Memory::new().get("anything"), None);
    }

    #[test]
    fn what_is_remembered_can_be_recalled_and_forgotten() {
        let mut memory = Memory::new();
        memory.remember("fridge", Recall::Cell(Point::new(3, 4)));
        assert_eq!(memory.get("fridge"), Some(&Recall::Cell(Point::new(3, 4))));

        memory.remember("fridge", Recall::Cell(Point::new(5, 6)));
        assert_eq!(memory.len(), 1, "remembering a key again replaces it");

        assert_eq!(memory.forget("fridge"), Some(Recall::Cell(Point::new(5, 6))));
        assert!(memory.is_empty());
    }

    #[test]
    fn memory_is_iterated_in_key_order_whatever_order_it_was_written_in() {
        // The determinism argument for a `BTreeMap`, stated as a test.
        let mut memory = Memory::new();
        for key in ["zebra", "apple", "mango"] {
            memory.remember(key, Recall::Number(1.0));
        }
        let keys: Vec<&str> = memory.iter().map(|(key, _)| key).collect();
        assert_eq!(keys, ["apple", "mango", "zebra"]);
    }

    #[test]
    fn a_place_out_of_reach_is_remembered_and_then_fades() {
        let mut places = OutOfReach::default();
        let toilet = Point::new(3, 4);
        places.remember(toilet, 100.0);
        assert!(places.contains(toilet, 100.0));
        assert!(places.contains(toilet, 100.0 + OUT_OF_REACH_FOR - 1.0));
        assert!(!places.contains(toilet, 100.0 + OUT_OF_REACH_FOR), "forgiven after a while");
        assert!(!places.contains(Point::new(4, 4), 100.0), "only the place itself");
        assert_eq!(places.iter(100.0 + OUT_OF_REACH_FOR).count(), 0);
    }

    #[test]
    fn remembering_a_place_again_extends_it_rather_than_taking_a_second_slot() {
        let mut places = OutOfReach::default();
        let toilet = Point::new(3, 4);
        places.remember(toilet, 0.0);
        places.remember(toilet, 50.0);
        assert_eq!(places.iter(0.0).count(), 1);
        assert!(places.contains(toilet, OUT_OF_REACH_FOR + 10.0));
    }

    #[test]
    fn a_full_memory_of_places_loses_the_one_forgiven_soonest() {
        let mut places = OutOfReach::default();
        for i in 0..OUT_OF_REACH_SLOTS as i32 {
            places.remember(Point::new(i, 0), 10.0 - i as f64);
        }
        // The last one written is the one forgiven soonest.
        let soonest = Point::new(OUT_OF_REACH_SLOTS as i32 - 1, 0);
        places.remember(Point::new(99, 0), 100.0);
        assert!(!places.contains(soonest, 0.0), "the one forgiven soonest went");
        for i in 0..OUT_OF_REACH_SLOTS as i32 - 1 {
            assert!(places.contains(Point::new(i, 0), 0.0), "{i} should still be remembered");
        }
        assert!(places.contains(Point::new(99, 0), 100.0));
    }
}
