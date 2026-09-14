//! What a brain remembers: [`Memory`].

use std::collections::BTreeMap;

use crate::map::Point;
use crate::sim::uid::Uid;

/// What a brain remembers. **Empty on purpose: nothing writes to it yet.**
///
/// A `BTreeMap` rather than a `HashMap`, and that is the only liberty taken
/// with the shape: iteration order has to be a declaration of the keys and not
/// an artefact of a hasher, or the day something iterates memory to make a
/// decision the thousand-tick determinism test starts failing once in every
/// few runs. Both allocate nothing while empty, which is today and for a while.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Memory(BTreeMap<String, Recall>);

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
        self.0.get(key)
    }

    /// Remember something under `key`, replacing whatever was there.
    pub fn remember(&mut self, key: impl Into<String>, recall: Recall) {
        self.0.insert(key.into(), recall);
    }

    /// Forget `key`; returns what it held.
    pub fn forget(&mut self, key: &str) -> Option<Recall> {
        self.0.remove(key)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// In key order, which is the whole reason this is a `BTreeMap`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Recall)> {
        self.0.iter().map(|(key, recall)| (key.as_str(), recall))
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
}
