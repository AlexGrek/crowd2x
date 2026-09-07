//! The entity store: a dictionary keyed by [`Uid`], over a dense arena.
//!
//! Callers see a dict — `get(uid)`, `insert`, `remove`, `iter` — because that
//! is how the rest of the game thinks about an entity: it has an id, and you
//! look it up. The tick does not use that door. It walks [`Entities::slots`]
//! in order and hashes nothing, which is the whole reason the storage is a
//! `Vec` and the `HashMap` is only an index into it. A `HashMap<Uid, Entity>`
//! iterated per agent per frame is one of the three mistakes that killed the
//! earlier prototype; see the `dev` skill.
//!
//! # Why tombstones rather than `swap_remove`
//!
//! A slot index has to be stable for as long as an entity lives:
//!
//! * The intent buffer in [`super::process_game_state`] is indexed by slot, so
//!   think and apply agree on which entity a decision belongs to without
//!   carrying an id through.
//! * Iteration order has to be the same on every run given the same history,
//!   or the simulation stops being deterministic. `swap_remove` reorders the
//!   tail on every despawn.
//!
//! So a removed entity leaves a `None` behind and its index goes on a free
//! list to be handed to the next spawn. The cost is that iteration skips
//! holes; the arena only grows to the high-water mark of live entities, which
//! for a crowd is the number that matters anyway.

use std::collections::HashMap;

use super::entity::GameEntity;
use super::uid::Uid;

/// Index of a slot in the arena. Stable for the lifetime of the entity in it.
pub type Slot = u32;

#[derive(Default)]
pub struct Entities {
    /// Dense and stable: a `None` is a hole waiting on the free list.
    slots: Vec<Option<Box<dyn GameEntity>>>,
    index: HashMap<Uid, Slot>,
    free: Vec<Slot>,
    live: usize,
}

impl Entities {
    pub fn new() -> Entities {
        Entities::default()
    }

    /// How many entities are alive. Not the arena length — see the module docs.
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// How many slots the arena holds, holes included. The bound on
    /// [`Entities::slot`] and the length the intent buffer is sized to.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub fn contains(&self, uid: Uid) -> bool {
        self.index.contains_key(&uid)
    }

    pub fn get(&self, uid: Uid) -> Option<&dyn GameEntity> {
        let slot = *self.index.get(&uid)?;
        self.slot(slot)
    }

    pub fn get_mut(&mut self, uid: Uid) -> Option<&mut (dyn GameEntity + 'static)> {
        let slot = *self.index.get(&uid)?;
        self.slot_mut(slot)
    }

    /// The entity in a slot, or `None` if that slot is a hole.
    pub fn slot(&self, slot: Slot) -> Option<&dyn GameEntity> {
        self.slots.get(slot as usize)?.as_deref()
    }

    pub fn slot_mut(&mut self, slot: Slot) -> Option<&mut (dyn GameEntity + 'static)> {
        match self.slots.get_mut(slot as usize)? {
            Some(entity) => Some(&mut **entity),
            None => None,
        }
    }

    pub fn slot_of(&self, uid: Uid) -> Option<Slot> {
        self.index.get(&uid).copied()
    }

    /// Add an entity, reusing a hole if there is one.
    ///
    /// Returns the slot it landed in. Panics on a duplicate id: ids come from
    /// [`super::GameState::mint_uid`], which retries until it has one nobody
    /// holds, so a collision here means that allocator is broken and quietly
    /// overwriting a live entity would hide it.
    pub fn insert(&mut self, entity: Box<dyn GameEntity>) -> Slot {
        let uid = entity.uid();
        assert!(
            !self.index.contains_key(&uid),
            "{uid} is already in the world"
        );

        let slot = match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = Some(entity);
                slot
            }
            None => {
                self.slots.push(Some(entity));
                (self.slots.len() - 1) as Slot
            }
        };
        self.index.insert(uid, slot);
        self.live += 1;
        slot
    }

    /// Take an entity out. Returns it, so a caller can log what went.
    pub fn remove(&mut self, uid: Uid) -> Option<Box<dyn GameEntity>> {
        let slot = self.index.remove(&uid)?;
        let entity = self.slots[slot as usize].take();
        if entity.is_some() {
            self.free.push(slot);
            self.live -= 1;
        }
        entity
    }

    /// Every live entity, in slot order — stable across a despawn, so two runs
    /// of the same history visit them in the same sequence.
    pub fn iter(&self) -> impl Iterator<Item = &dyn GameEntity> {
        self.slots.iter().filter_map(|slot| slot.as_deref())
    }

    /// Every live entity with its slot.
    pub fn iter_slots(&self) -> impl Iterator<Item = (Slot, &dyn GameEntity)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| Some((index as Slot, slot.as_deref()?)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut (dyn GameEntity + 'static)> {
        self.slots
            .iter_mut()
            .filter_map(|slot| slot.as_mut().map(|entity| &mut **entity))
    }

    /// Every live id, in slot order.
    pub fn uids(&self) -> impl Iterator<Item = Uid> + '_ {
        self.iter().map(|entity| entity.uid())
    }
}

impl std::fmt::Debug for Entities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entities")
            .field("live", &self.live)
            .field("slots", &self.slots.len())
            .field("free", &self.free.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::entity::{Body, Think};
    use crate::sim::uid::EntityType;
    use crate::sim::Intent;

    struct Stub(Body);

    impl GameEntity for Stub {
        fn body(&self) -> &Body {
            &self.0
        }
        fn body_mut(&mut self) -> &mut Body {
            &mut self.0
        }
        fn think(&self, _ctx: &Think<'_>) -> Intent {
            Intent::Idle
        }
    }

    fn stub(body: u64) -> Box<dyn GameEntity> {
        let uid = Uid::new(EntityType::Human, body);
        Box::new(Stub(Body::new(uid, (0.0, 0.0))))
    }

    fn uid(body: u64) -> Uid {
        Uid::new(EntityType::Human, body)
    }

    #[test]
    fn an_entity_is_found_by_its_id() {
        let mut entities = Entities::new();
        entities.insert(stub(1));
        entities.insert(stub(2));

        assert_eq!(entities.len(), 2);
        assert_eq!(entities.get(uid(1)).map(|e| e.uid()), Some(uid(1)));
        assert!(entities.contains(uid(2)));
        assert!(entities.get(uid(3)).is_none());
    }

    #[test]
    fn removing_an_entity_takes_it_out_of_both_the_arena_and_the_index() {
        let mut entities = Entities::new();
        entities.insert(stub(1));
        let gone = entities.remove(uid(1)).expect("it was there");

        assert_eq!(gone.uid(), uid(1));
        assert_eq!(entities.len(), 0);
        assert!(!entities.contains(uid(1)));
        assert!(entities.get(uid(1)).is_none());
        // Removing it twice is not an error, and does not decrement past zero.
        assert!(entities.remove(uid(1)).is_none());
        assert_eq!(entities.len(), 0);
    }

    #[test]
    fn a_despawn_leaves_the_other_slots_where_they_were() {
        // `swap_remove` would move the last entity into the hole, changing the
        // slot the intent buffer indexes by and the order iteration visits.
        let mut entities = Entities::new();
        for body in 1..=3 {
            entities.insert(stub(body));
        }
        let slot_of_three = entities.slot_of(uid(3)).expect("inserted");

        entities.remove(uid(2));

        assert_eq!(entities.slot_of(uid(3)), Some(slot_of_three));
        let order: Vec<Uid> = entities.uids().collect();
        assert_eq!(order, [uid(1), uid(3)]);
    }

    #[test]
    fn a_hole_is_reused_before_the_arena_grows() {
        let mut entities = Entities::new();
        for body in 1..=3 {
            entities.insert(stub(body));
        }
        assert_eq!(entities.capacity(), 3);

        entities.remove(uid(2));
        let slot = entities.insert(stub(4));

        assert_eq!(slot, 1, "the hole should have been filled");
        assert_eq!(entities.capacity(), 3, "the arena should not have grown");
        assert_eq!(entities.len(), 3);
    }

    #[test]
    fn iteration_skips_the_holes() {
        let mut entities = Entities::new();
        for body in 1..=4 {
            entities.insert(stub(body));
        }
        entities.remove(uid(1));
        entities.remove(uid(4));

        assert_eq!(entities.iter().count(), 2);
        let slots: Vec<Slot> = entities.iter_slots().map(|(slot, _)| slot).collect();
        assert_eq!(slots, [1, 2]);
    }

    #[test]
    #[should_panic(expected = "already in the world")]
    fn inserting_the_same_id_twice_is_a_bug_rather_than_an_overwrite() {
        let mut entities = Entities::new();
        entities.insert(stub(1));
        entities.insert(stub(1));
    }
}
