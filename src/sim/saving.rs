//! The world in a save file (`crate::save`): what of a [`GameState`] is kept,
//! and how a world is built back from it.
//!
//! **A world that is saved and loaded carries on exactly as it would have.**
//! Not roughly, not "a world like it": the same positions, to the bit, on
//! every tick after, which is what `crate::save`'s tests run two worlds side
//! by side to check. So everything that decides what happens next is kept —
//! the id allocator's state, every slot of the arena and its holes, who holds
//! which cell, a search the route budget stopped halfway — and everything
//! that can be worked out again from the map is worked out again rather than
//! trusted: features, properties, the supply, which doors are locked.
//!
//! What is *not* kept is what a tick starts by overwriting (the intent, move
//! and effect buffers), what only says how memory is used (which chunks the
//! census and the occupancy layer hold, where the reclaiming sweeps had got
//! to), and the log.
//!
//! A child of `sim` rather than a part of `crate::save`, because what it
//! reads are `GameState`'s own fields; `crate::save` owns the file.

use rayon::prelude::*;

use crate::map::{Map, Point};
use crate::save::{invalid, pb, Invalid};

use super::entities::{Entities, Slot};
use super::entity::GameEntity;
use super::homes::Homes;
use super::job::Workplaces;
use super::{kinds, rng, Effect, GameState, Intent, MoveOutcome};

/// One entity read back from a save, before it is put in its world.
pub struct RestoredEntity {
    slot: Slot,
    serial: u64,
    holds_cell: bool,
    entity: Box<dyn GameEntity>,
}

impl RestoredEntity {
    /// The entity a save's slot holds, standing beside a map whose navigation
    /// is at `revision` ([`crate::sim::Walker`] restores a search against
    /// it). Self-contained, so a save's shards are restored in parallel.
    pub fn restore(record: &pb::SlotRecord, revision: u64) -> Result<RestoredEntity, Invalid> {
        let kind = record
            .kind
            .as_ref()
            .ok_or_else(|| Invalid::new(format!("slot {} holds no kind of entity", record.slot)))?;
        let entity = kinds::restore(kind, revision).map_err(|error| error.within(format!("slot {}", record.slot)))?;
        Ok(RestoredEntity { slot: record.slot, serial: record.serial, holds_cell: record.holds_cell, entity })
    }
}

impl GameState {
    /// Everything about the world that is not an entity, the map or a
    /// chunk.
    pub fn save_world(&self) -> pb::World {
        pb::World {
            tick: self.tick,
            elapsed: self.elapsed,
            rng: rng::state_of(&self.rng).to_vec(),
            boxes_off: pb::pack(self.boxes_off.iter().copied()),
            street_lights_on: self.street_lights_on,
            switches_off: pb::pack(self.light_switches.cells().filter(|&cell| !self.light_switches.is_on(cell))),
            fridges: self.fridges.save(),
            bins: self.bins.save(),
            doors: self.doors.save(),
            homes: self.homes.save(),
            arena: Some(self.entities.save_arena()),
            workplaces: self.workplaces.save(),
            tally: Some(self.tally.save()),
        }
    }

    /// The arena, `per_file` slots a shard — holes included in the count,
    /// so a slot's shard is fixed by its number — each shard handed to
    /// `encode` on whichever thread made it. In slot order.
    ///
    /// Across threads because a crowd of thousands is most of a save, and
    /// reading it is read-only: each shard reads only its own slots.
    pub fn save_entities<T: Send>(&self, per_file: usize, encode: impl Fn(pb::EntityShard) -> T + Sync) -> Vec<T> {
        let capacity = self.entities.capacity();
        let per_file = per_file.max(1);
        (0..capacity.div_ceil(per_file))
            .into_par_iter()
            .map(|shard| {
                let slots = (shard * per_file) as Slot..((shard + 1) * per_file).min(capacity) as Slot;
                let records = slots
                    .filter_map(|slot| {
                        let entity = self.entities.slot(slot)?;
                        Some(pb::SlotRecord {
                            slot,
                            serial: self.entities.serial(slot),
                            holds_cell: self.occupancy.occupant(entity.center_position()) == Some(entity.uid()),
                            kind: Some(entity.save()),
                        })
                    })
                    .collect();
                encode(pb::EntityShard { slots: records })
            })
            .collect()
    }

    /// A world built back from a save: its site and chunks already restored
    /// into `map`, its own state from `world`, and its entities, each back in
    /// its own slot.
    ///
    /// Built the way a new world is ([`GameState::new`]) — so everything that
    /// follows from the map follows from it again — and then given the state
    /// that was saved: the clock, the id allocator, the switches and so the
    /// supply, every fridge and door, the beds, and the crowd, each holding
    /// the cell it held.
    ///
    /// Neither prepares the chunks round the crowd nor reclaims any: the
    /// world was saved between ticks, and the first spawn pass after a load
    /// does both, as it would have.
    pub fn restore(map: Map, world: &pb::World, entities: Vec<RestoredEntity>) -> Result<GameState, Invalid> {
        let revision = map.navigation_revision();
        let mut state = GameState::new(map, 0);
        // What the restored searches were stamped with. Building a world
        // loads chunks and works out reachability, neither of which moves it.
        debug_assert_eq!(state.map.navigation_revision(), revision);

        state.rng = rng::from_state(&world.rng)?;
        if !(world.elapsed.is_finite() && world.elapsed >= 0.0) {
            return invalid(format!("{} seconds since the world began", world.elapsed));
        }
        state.tick = world.tick;
        state.elapsed = world.elapsed;

        let mut boxes_off = pb::unpack(&world.boxes_off, "boxes off")?;
        boxes_off.sort_unstable();
        boxes_off.dedup();
        if let Some(cell) = boxes_off.iter().find(|&&cell| !state.has_box(cell)) {
            return invalid(format!("no distribution box at {}, {} to be off", cell.x, cell.y));
        }
        state.boxes_off = boxes_off;
        state.street_lights_on = world.street_lights_on;
        for cell in pb::unpack(&world.switches_off, "switches off")? {
            if state.light_switches.switch(cell, false).is_none() {
                return invalid(format!("no light switch at {}, {} to be off", cell.x, cell.y));
            }
        }
        // The supply both ways, the features and the fridges' power, for
        // the switches as saved.
        state.refresh_supply();
        state.fridges.restore(&world.fridges)?;
        state.bins.restore(&world.bins)?;
        state.tally = crate::sim::statistics::Tally::restore(world.tally.as_ref());
        state.doors.restore(&world.doors)?;
        state.homes = Homes::restore(&world.homes)?;
        state.workplaces = Workplaces::restore(&world.workplaces)?;

        let arena = pb::required(&world.arena, "arena")?;
        let mut holders: Vec<(Point, crate::sim::Uid)> = Vec::new();
        state.entities = Entities::restore(
            arena,
            entities.into_iter().map(|restored| {
                if restored.holds_cell {
                    holders.push((restored.entity.center_position(), restored.entity.uid()));
                }
                (restored.slot, restored.serial, restored.entity)
            }),
        )?;

        let capacity = state.entities.capacity();
        state.intents = vec![Intent::Idle; capacity];
        state.moves = vec![MoveOutcome::Idle; capacity];
        state.effects = vec![Effect::None; capacity];
        for (slot, entity) in state.entities.iter_slots() {
            state.census.file(slot, entity.center_position());
        }
        state.corpses = crate::sim::Corpses::of(&state.entities);
        for (cell, uid) in holders {
            if let Err(other) = state.occupancy.claim(cell, uid) {
                return invalid(format!("{uid} and {other} both hold {}, {}", cell.x, cell.y));
            }
        }
        // A bed is given to somebody who arrived, and given back when they
        // leave: its owner is in the world.
        for home in &world.homes {
            let here = crate::sim::Uid::from_raw(home.owner).is_some_and(|owner| state.entities.contains(owner));
            if !here {
                return invalid(format!("a bed belongs to {:#x}, who is not here", home.owner));
            }
        }
        // A desk is given with a job, and taken back with it: whoever works
        // at a desk has the job that says so, and everybody with a job has
        // its desk.
        for (_, entity) in state.entities.iter_slots() {
            if let Some(job) = entity.job()
                && state.workplaces.worker(job.workplace, job.shift) != Some(entity.uid())
            {
                let at = job.workplace;
                return invalid(format!("{} works at {}, {}, which is not their desk", entity.uid(), at.x, at.y));
            }
        }
        for place in &world.workplaces {
            let worker = crate::sim::Uid::from_raw(place.worker);
            let works_here = worker.and_then(|uid| state.entities.get(uid)).and_then(|entity| entity.job());
            let start = place.start.unwrap_or(crate::sim::job::OFFICE_HOURS.start() as u32);
            if works_here.map(|job| (Some(pb::Cell::from(job.workplace)), job.shift.start() as u32)) != Some((place.desk, start)) {
                return invalid(format!("a desk belongs to {:#x}, who does not work there", place.worker));
            }
        }
        Ok(state)
    }
}
