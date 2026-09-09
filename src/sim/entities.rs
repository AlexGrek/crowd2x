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
use std::ops::Deref;

use rayon::prelude::*;

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

    /// Every live entity with its slot, mutably.
    ///
    /// This is what the apply phase walks. The obvious alternative — loop over
    /// `0..capacity()` and call [`Entities::slot_mut`] — pays a bounds check
    /// and an `Option` test per slot to rediscover what the iterator already
    /// knows, and visits every hole to do it.
    pub fn iter_slots_mut(
        &mut self,
    ) -> impl Iterator<Item = (Slot, &mut (dyn GameEntity + 'static))> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| Some((index as Slot, &mut **slot.as_mut()?)))
    }

    /// The arena as a parallel iterator, **holes included**: item `i` is the
    /// entity in slot `i`, or `None`.
    ///
    /// Indexed, and that is what it is for — the caller zips it with a
    /// slot-indexed buffer and gets a guarantee from the type system that the
    /// two are lined up. The alternative, filtering the holes out first, is
    /// not indexed, so a slot number would have to be carried alongside and
    /// the buffer indexed by hand from several threads at once.
    pub fn par_iter_slots(
        &self,
    ) -> impl IndexedParallelIterator<Item = Option<&(dyn GameEntity + 'static)>> {
        self.slots.par_iter().map(|slot| slot.as_deref())
    }

    /// [`Entities::par_iter_slots`], mutably.
    ///
    /// Each item is a distinct entity, so a thread that has one cannot reach
    /// another — which is the whole of what makes the think and react rounds
    /// safe to run in parallel. `Box<dyn GameEntity>` is `Send + Sync` because
    /// [`GameEntity`] says so.
    pub fn par_iter_slots_mut(
        &mut self,
    ) -> impl IndexedParallelIterator<Item = Option<&mut (dyn GameEntity + 'static)>> {
        self.slots.par_iter_mut().map(|slot| slot.as_deref_mut())
    }

    /// Every live id, in slot order.
    pub fn uids(&self) -> impl Iterator<Item = Uid> + '_ {
        self.iter().map(|entity| entity.uid())
    }

    /// Hand the table to the processing pass with its membership frozen.
    ///
    /// See [`FrozenEntities`]. The point is that the processing pass takes
    /// this and not `&mut Entities`, so "processing does not write to the
    /// entity table" stops being a convention somebody has to remember.
    pub fn freeze(&mut self) -> FrozenEntities<'_> {
        FrozenEntities(self)
    }
}

/// The entity table with its **membership frozen**: entities may change, but
/// which entities exist may not.
///
/// This is what the processing pass is given. It cannot spawn and it cannot
/// despawn, because it has no `&mut Entities` to call [`Entities::insert`] or
/// [`Entities::remove`] on — only this, which does not offer them.
///
/// That is worth a type rather than a comment, because three things quietly
/// depend on the table's shape holding still for the whole of a tick:
///
/// * The intent buffer is indexed by [`Slot`] and sized once, in the spawn
///   pass. An insert mid-tick would grow the arena past the buffer.
/// * A `Vec` that grows moves its contents. Anything holding a reference into
///   the table across a spawn would be holding a dangling one — which the
///   borrow checker stops today only because the code happens to be written so
///   it never tries.
/// * The processing pass is meant to become parallel. Structural mutation is
///   precisely what cannot be parallelised; moving entities cannot conflict,
///   inserting into a shared `Vec` always does.
///
/// Read-only access comes through `Deref`, so everything that only needs
/// `&Entities` works unchanged. There is deliberately **no `DerefMut`** — that
/// would hand back the two methods this type exists to withhold.
///
/// An entity that needs to remove itself does not get a back door here: it
/// returns something the *next* spawn pass acts on, the same way a spawn
/// arrives as a [`super::Command`].
pub struct FrozenEntities<'a>(&'a mut Entities);

impl FrozenEntities<'_> {
    /// The entity in a slot, mutably. See [`Entities::iter_slots_mut`].
    pub fn iter_slots_mut(
        &mut self,
    ) -> impl Iterator<Item = (Slot, &mut (dyn GameEntity + 'static))> {
        self.0.iter_slots_mut()
    }

    /// [`Entities::par_iter_slots_mut`]. See [`react_step`] for what walks it.
    ///
    /// [`react_step`]: super::react_step
    pub fn par_iter_slots_mut(
        &mut self,
    ) -> impl IndexedParallelIterator<Item = Option<&mut (dyn GameEntity + 'static)>> {
        self.0.par_iter_slots_mut()
    }

    pub fn get_mut(&mut self, uid: Uid) -> Option<&mut (dyn GameEntity + 'static)> {
        self.0.get_mut(uid)
    }
}

/// Every read-only method of [`Entities`], and none of the structural ones.
impl Deref for FrozenEntities<'_> {
    type Target = Entities;

    fn deref(&self) -> &Entities {
        self.0
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

    #[test]
    fn a_frozen_table_still_reads_like_the_table_it_came_from() {
        let mut entities = Entities::new();
        entities.insert(stub(1));
        entities.insert(stub(2));

        let mut frozen = entities.freeze();
        // Everything read-only arrives through `Deref`.
        assert_eq!(frozen.len(), 2);
        assert_eq!(frozen.capacity(), 2);
        assert!(frozen.contains(uid(1)));
        assert_eq!(frozen.iter().count(), 2);
        // ...and entities can still be moved, just not added or removed.
        assert_eq!(frozen.iter_slots_mut().count(), 2);
        assert!(frozen.get_mut(uid(2)).is_some());
    }

    /// `insert` and `remove` take `&mut Entities`, and `FrozenEntities` offers
    /// only `Deref` — so neither is reachable through it. Compiling this file
    /// with the lines below uncommented is what would break:
    ///
    /// ```compile_fail
    /// let mut entities = crowd2x::sim::Entities::new();
    /// let frozen = entities.freeze();
    /// frozen.remove(uid);          // no method `remove`
    /// ```
    ///
    /// A doctest cannot run against a binary crate, so this is a note rather
    /// than a check. The guarantee is the absence of `DerefMut`, which is
    /// visible in one place directly above.
    #[test]
    fn a_frozen_table_offers_no_way_to_change_who_exists() {
        let mut entities = Entities::new();
        entities.insert(stub(1));
        let frozen = entities.freeze();
        assert_eq!(frozen.len(), 1);
    }
}
