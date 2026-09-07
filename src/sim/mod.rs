//! The simulation: one super-object, and one function that advances it.
//!
//! **Nothing in this module imports `bevy`, and nothing should start to.** The
//! game runs here. Bevy renders it, reads the devices, runs the shaders and
//! draws the interface — that is the whole of its job. `src/map/` already
//! follows this rule; this is the same rule one level up, over the state that
//! changes.
//!
//! # The shape
//!
//! ```text
//! GameState { map, entities, log }
//!      |
//!      +-- process_game_state(&mut state, dt, &input)
//! ```
//!
//! One state, one entry point. Not a plugin, not a set of systems, not a
//! schedule — a function you can call in a test, in a loop, a thousand times
//! with no `App` anywhere. That is the point: the earlier prototype welded its
//! logic to its engine's API and could not be tested, profiled or ported
//! without it.
//!
//! ## Why `&mut` and not `-> GameState`
//!
//! The name says `process_game_state(state) -> GameState`, and the honest
//! implementation of that is this signature. Returning a fresh state means
//! copying the map, the log queue and the id allocator sixty times a second to
//! express a change that touches only the entities. The *semantics* asked for
//! — a tick is a pure function of the previous state and the input — are
//! delivered by the phase split below, which is where they actually matter.
//!
//! ## The tick
//!
//! 1. **Commands.** Drain [`Input::commands`]: spawns and despawns, the only
//!    things that change how many entities there are.
//! 2. **Think.** Read-only over the entities and the map. Every entity returns
//!    an [`Intent`] into a slot-indexed buffer. Nothing is mutated, so there
//!    is nothing to synchronise and this loop can be handed to a thread pool
//!    without changing its shape.
//! 3. **Apply.** Single-threaded, slots in ascending order, drains the buffer
//!    into the entities.
//!
//! **The intent buffer is the double buffer.** The `dev` skill asks for
//! read-from-A-write-to-B, and this is that, without cloning an entity: think
//! reads the entities and writes intents, apply reads intents and writes the
//! entities. The two phases never touch the same memory in the same direction,
//! which is the property that makes the read phase safe to parallelise.
//!
//! # Lock-free
//!
//! There is no `Mutex` here and there is not meant to be one. That is not
//! achieved with atomics — it is achieved by there being no shared mutable
//! state during the phase that runs concurrently. The one thing many threads
//! genuinely write at once is [`Log`], which is a bounded lock-free queue
//! precisely so it does not become the global lock that killed the earlier
//! prototype.

// The simulation is being built out ahead of its callers — most of the public
// surface here is reached from tests and from the Bevy adapter, and trimming
// it to whatever `src/game/` happens to touch today would mean re-adding it a
// piece at a time. Same reasoning as `src/map/`.
#![allow(dead_code)]

pub mod entities;
pub mod entity;
pub mod kinds;
pub mod log;
pub mod uid;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use crate::map::{Map, Point};

pub use entities::{Entities, Slot};
pub use entity::{Body, GameEntity, Think};
pub use kinds::{Dog, Facing, Human};
pub use log::Log;
pub use uid::{EntityType, Uid};

/// What an entity decided to do this tick.
///
/// Deliberately small and `Copy`: there is one of these per entity per tick,
/// they live in a buffer that is reused rather than reallocated, and anything
/// that needs a heap allocation to express is a sign the decision should have
/// been split into more ticks.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Intent {
    /// Do nothing. Also what an entity with nowhere to go returns.
    #[default]
    Idle,
    /// Be here at the end of the tick, and hold this goal afterwards.
    ///
    /// The goal rides along because [`GameEntity::think`] cannot write it: an
    /// entity that picks somewhere new to walk has to say so through the one
    /// channel the read phase is allowed to use.
    Move {
        to: (f32, f32),
        goal: Option<Point>,
    },
}

/// Something done *to* the world, from outside it.
///
/// Separate from [`Intent`] on purpose: an intent is what an entity wants, a
/// command is what the player, the debug harness or a QA script did. The two
/// arrive by different routes and are applied in different phases.
#[derive(Clone, PartialEq, Debug)]
pub enum Command {
    Spawn { kind: EntityType, at: Point },
    Despawn(Uid),
}

/// One frame's worth of input to the simulation.
///
/// A struct rather than loose arguments so that giving the simulation
/// something new to read does not change [`process_game_state`]'s signature
/// and every call site with it.
#[derive(Clone, Default, Debug)]
pub struct Input {
    /// Applied in order, before anything thinks.
    pub commands: Vec<Command>,
    /// Where the pointer is, in cells, if it is over the map at all.
    pub cursor: Option<Point>,
}

impl Input {
    pub fn new() -> Input {
        Input::default()
    }

    pub fn spawn(&mut self, kind: EntityType, at: Point) -> &mut Input {
        self.commands.push(Command::Spawn { kind, at });
        self
    }

    pub fn despawn(&mut self, uid: Uid) -> &mut Input {
        self.commands.push(Command::Despawn(uid));
        self
    }

    /// Empty the command list, keeping the allocation.
    ///
    /// Commands are consumed by a tick; leaving them in place would replay
    /// them every frame, which for a spawn means an unbounded crowd.
    pub fn clear(&mut self) {
        self.commands.clear();
    }
}

/// The whole game.
///
/// One object, held by one owner. The Bevy side keeps it in a resource and
/// exactly one system takes it mutably — every `ResMut<T>` is an exclusive
/// lock on Bevy's schedule, so a second writer would serialise the frame
/// around the simulation for no benefit.
pub struct GameState {
    /// The static world. Owned rather than borrowed: a `GameState` outlives
    /// whatever loaded the map, and a lifetime here would infect every caller.
    pub map: Map,
    entities: Entities,
    /// Written from any thread, drained by the renderer. See [`Log`].
    pub log: Log,
    /// Mints ids and seeds new entities. Seeded, so a run replays.
    rng: SmallRng,
    tick: u64,
    elapsed: f64,
    /// The back buffer, reused between ticks so a tick allocates nothing.
    /// Indexed by [`Slot`]; a hole in the arena keeps an [`Intent::Idle`].
    intents: Vec<Intent>,
}

impl GameState {
    /// An empty world on `map`.
    ///
    /// No entities: [`GameState::spawn`] is the only way one arrives, so a
    /// world is exactly what its caller asked for and nothing appears by
    /// default.
    pub fn new(map: Map, seed: u64) -> GameState {
        GameState {
            map,
            entities: Entities::new(),
            log: Log::new(),
            rng: SmallRng::seed_from_u64(seed),
            tick: 0,
            elapsed: 0.0,
            intents: Vec::new(),
        }
    }

    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    /// How many entities are alive.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// How many ticks have been processed.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Simulated seconds since the world was created. `f64` because at 60Hz an
    /// `f32` stops being able to represent whole ticks after a few hours.
    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }

    /// An id nobody in this world holds.
    ///
    /// "Guaranteed unique" is this retry, not a hope about 56 random bits: the
    /// bits make a collision vanishingly unlikely, and the loop makes it
    /// impossible. Bounded so a broken RNG returning a constant fails loudly
    /// instead of hanging the game.
    fn mint_uid(&mut self, kind: EntityType) -> Uid {
        for _ in 0..64 {
            let uid = Uid::new(kind, self.rng.random());
            if !self.entities.contains(uid) {
                return uid;
            }
        }
        panic!("could not mint a free {kind} id in 64 tries — is the rng stuck?");
    }

    /// Put a new entity of `kind` in the middle of `at`.
    ///
    /// Placed wherever it is asked for, passable or not: a caller that wants a
    /// legal spawn point should pick one, and silently moving an entity
    /// somewhere else would make a test that spawns at a known cell lie.
    pub fn spawn(&mut self, kind: EntityType, at: Point) -> Uid {
        let uid = self.mint_uid(kind);
        let entity = {
            // Split off a per-entity stream so the shape of an entity does not
            // depend on how many were made before it.
            let mut rng = SmallRng::seed_from_u64(self.rng.random());
            kinds::build(uid, kind, at, &mut rng)
        };
        let slot = self.entities.insert(entity);

        // The buffer is indexed by slot, so it has to cover the new one.
        if self.intents.len() <= slot as usize {
            self.intents.resize(slot as usize + 1, Intent::Idle);
        }
        self.intents[slot as usize] = Intent::Idle;

        self.log.push(format!("spawned {uid} at {},{}", at.x, at.y));
        uid
    }

    /// Remove an entity. Returns whether there was one.
    pub fn despawn(&mut self, uid: Uid) -> bool {
        match self.entities.remove(uid) {
            Some(_) => {
                self.log.push(format!("despawned {uid}"));
                true
            }
            None => false,
        }
    }

    /// Where an entity is, in cell units.
    pub fn position_of(&self, uid: Uid) -> Option<(f32, f32)> {
        self.entities.get(uid).map(|entity| entity.position())
    }
}

impl std::fmt::Debug for GameState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameState")
            .field("tick", &self.tick)
            .field("entities", &self.entities.len())
            .field("map", &self.map)
            .field("log", &self.log)
            .finish()
    }
}

/// Advance the world by `dt` seconds.
///
/// The one function. See the module docs for the three phases and for why this
/// takes `&mut` rather than returning a new state.
pub fn process_game_state(state: &mut GameState, dt: f32, input: &Input) {
    apply_commands(state, input);

    state.tick += 1;
    state.elapsed += dt as f64;

    // Disjoint field borrows: `intents` is written while `entities`, `map` and
    // `log` are read. Taking the buffer out and putting it back would work too,
    // and would allocate nothing either, but this way the borrow checker is
    // the thing proving the phases do not overlap.
    let GameState {
        map,
        entities,
        log,
        tick,
        intents,
        ..
    } = state;

    // Sized to the arena, not the live count: it is indexed by slot, and a
    // hole keeps an `Idle` that apply skips.
    intents.resize(entities.capacity(), Intent::Idle);

    // --- think: read-only, and shaped to be run in parallel ---
    let ctx = Think {
        map,
        log,
        dt,
        tick: *tick,
    };
    for (slot, entity) in entities.iter_slots() {
        intents[slot as usize] = entity.think(&ctx);
    }

    // --- apply: single-threaded, in slot order ---
    for slot in 0..entities.capacity() as Slot {
        let intent = intents[slot as usize];
        if intent == Intent::Idle {
            continue;
        }
        if let Some(entity) = entities.slot_mut(slot) {
            entity.apply(&intent);
        }
    }
}

/// Spawns and despawns, before anything thinks.
///
/// Ahead of the tick so that an entity spawned this frame thinks this frame:
/// spawning something and watching it stand still for a tick reads as a bug in
/// whatever did the spawning.
fn apply_commands(state: &mut GameState, input: &Input) {
    for command in &input.commands {
        match command {
            Command::Spawn { kind, at } => {
                state.spawn(*kind, *at);
            }
            Command::Despawn(uid) => {
                if !state.despawn(*uid) {
                    state.log.push(format!("despawn: no such entity {uid}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR, VOID, WALL};

    fn world() -> GameState {
        GameState::new(Map::new(Size::new(12, 12), FLOOR), 1)
    }

    fn run(state: &mut GameState, ticks: u32) {
        let input = Input::new();
        for _ in 0..ticks {
            process_game_state(state, 1.0 / 60.0, &input);
        }
    }

    #[test]
    fn a_new_world_is_empty() {
        let state = world();
        assert_eq!(state.len(), 0);
        assert_eq!(state.tick(), 0);
        assert_eq!(state.elapsed(), 0.0);
    }

    #[test]
    fn spawning_puts_an_entity_where_it_was_asked_for() {
        let mut state = world();
        let at = Point::new(3, 7);
        let uid = state.spawn(EntityType::Human, at);

        assert_eq!(state.len(), 1);
        assert_eq!(uid.kind(), Some(EntityType::Human));
        let entity = state.entities().get(uid).expect("just spawned");
        assert_eq!(entity.center_position(), at);
        assert_eq!(entity.position(), (3.5, 7.5));
    }

    #[test]
    fn every_id_is_unique_across_a_lot_of_spawning() {
        let mut state = world();
        let mut seen = std::collections::HashSet::new();
        for i in 0..10_000 {
            let kind = if i % 2 == 0 {
                EntityType::Human
            } else {
                EntityType::Dog
            };
            let uid = state.spawn(kind, Point::new(1, 1));
            assert!(seen.insert(uid), "{uid} was minted twice");
            assert_eq!(uid.kind(), Some(kind));
        }
        assert_eq!(state.len(), 10_000);
    }

    #[test]
    fn a_despawned_entity_is_gone_and_a_second_despawn_is_not_an_error() {
        let mut state = world();
        let uid = state.spawn(EntityType::Dog, Point::new(2, 2));

        assert!(state.despawn(uid));
        assert_eq!(state.len(), 0);
        assert!(state.entities().get(uid).is_none());
        assert!(!state.despawn(uid));
    }

    #[test]
    fn commands_are_applied_before_anything_thinks() {
        let mut state = world();
        let mut input = Input::new();
        input.spawn(EntityType::Human, Point::new(5, 5));

        process_game_state(&mut state, 1.0 / 60.0, &input);

        // Spawned *and* moved, in the one tick.
        assert_eq!(state.len(), 1);
        let entity = state.entities().iter().next().expect("spawned");
        assert_ne!(entity.position(), (5.5, 5.5));
    }

    #[test]
    fn a_despawn_command_for_nothing_says_so_rather_than_failing() {
        let mut state = world();
        let ghost = Uid::new(EntityType::Dog, 999);
        let mut input = Input::new();
        input.despawn(ghost);

        process_game_state(&mut state, 1.0 / 60.0, &input);

        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("no such entity")));
    }

    #[test]
    fn the_tick_counter_and_the_clock_advance_together() {
        let mut state = world();
        run(&mut state, 60);
        assert_eq!(state.tick(), 60);
        assert!((state.elapsed() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_same_seed_and_the_same_input_give_the_same_world() {
        // The property the whole think/apply split exists to protect. Without
        // it a bug reproduces once and never again.
        let build = || {
            let mut state = GameState::new(Map::new(Size::new(16, 16), FLOOR), 7);
            for i in 0..8 {
                state.spawn(EntityType::Human, Point::new(2 + i, 3));
                state.spawn(EntityType::Dog, Point::new(3, 2 + i));
            }
            state
        };

        let (mut a, mut b) = (build(), build());
        run(&mut a, 1000);
        run(&mut b, 1000);

        let positions = |state: &GameState| -> Vec<(u64, (f32, f32))> {
            state
                .entities()
                .iter()
                .map(|e| (e.uid().raw(), e.position()))
                .collect()
        };
        assert_eq!(positions(&a), positions(&b));
    }

    #[test]
    fn a_different_seed_gives_a_different_world() {
        let build = |seed| {
            let mut state = GameState::new(Map::new(Size::new(16, 16), FLOOR), seed);
            state.spawn(EntityType::Human, Point::new(8, 8));
            state
        };
        let (mut a, mut b) = (build(1), build(2));
        run(&mut a, 100);
        run(&mut b, 100);

        let pos = |s: &GameState| s.entities().iter().next().unwrap().position();
        assert_ne!(pos(&a), pos(&b));
    }

    #[test]
    fn nothing_ever_ends_a_tick_in_an_impassable_cell() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        for y in 0..10 {
            map.set_terrain(Point::new(5, y), WALL);
        }
        let mut state = GameState::new(map, 3);
        for i in 0..6 {
            state.spawn(EntityType::Human, Point::new(1 + i % 4, 1 + i));
        }

        for tick in 0..1000 {
            run(&mut state, 1);
            for entity in state.entities().iter() {
                assert!(
                    state.map.is_passable(entity.center_position()),
                    "{} is in {:?} on tick {tick}",
                    entity.uid(),
                    entity.center_position()
                );
            }
        }
    }

    #[test]
    fn a_world_with_nowhere_to_stand_ticks_without_moving_anything() {
        // Every cell VOID: the wanderer can never find a goal. It must idle,
        // not spin, drift or panic.
        let mut state = GameState::new(Map::empty(Size::new(8, 8)), 5);
        let uid = state.spawn(EntityType::Human, Point::new(4, 4));
        let before = state.position_of(uid);

        run(&mut state, 200);

        assert_eq!(state.position_of(uid), before);
    }

    #[test]
    fn despawning_mid_run_does_not_disturb_the_entities_that_remain() {
        // The tombstone rule, end to end: the slot buffer must stay lined up
        // with the arena across a removal.
        let mut a = GameState::new(Map::new(Size::new(16, 16), FLOOR), 11);
        let mut b = GameState::new(Map::new(Size::new(16, 16), FLOOR), 11);
        let mut keep = Vec::new();
        for state in [&mut a, &mut b] {
            let mut ids = Vec::new();
            for i in 0..5 {
                ids.push(state.spawn(EntityType::Human, Point::new(2 + i, 8)));
            }
            keep.push(ids);
        }

        run(&mut a, 50);
        // Only `b` loses its middle entity.
        run(&mut b, 25);
        b.despawn(keep[1][2]);
        run(&mut b, 25);

        for i in [0, 1, 3, 4] {
            assert_eq!(
                a.position_of(keep[0][i]),
                b.position_of(keep[1][i]),
                "entity {i} was disturbed by an unrelated despawn"
            );
        }
        assert_eq!(b.len(), 4);
    }

    #[test]
    fn a_reused_slot_belongs_entirely_to_its_new_entity() {
        // A stale intent left in a reused slot would move the newcomer on the
        // strength of a decision the dead entity made.
        let mut state = world();
        let first = state.spawn(EntityType::Human, Point::new(4, 4));
        run(&mut state, 10);
        state.despawn(first);

        let second = state.spawn(EntityType::Dog, Point::new(9, 9));
        assert_eq!(state.position_of(second), Some((9.5, 9.5)));
        assert_eq!(state.len(), 1);
    }

    #[test]
    fn the_log_records_what_came_and_went() {
        let mut state = world();
        let uid = state.spawn(EntityType::Dog, Point::new(1, 2));
        state.despawn(uid);

        let lines = state.log.drain();
        assert!(lines.iter().any(|l| l.contains("spawned") && l.contains("1,2")));
        assert!(lines.iter().any(|l| l.starts_with("despawned")));
    }

    #[test]
    fn a_tick_allocates_no_intent_buffer_after_the_first() {
        let mut state = world();
        for i in 0..10 {
            state.spawn(EntityType::Human, Point::new(1, 1 + i));
        }
        run(&mut state, 1);
        let capacity = state.intents.capacity();
        run(&mut state, 500);
        assert_eq!(state.intents.capacity(), capacity);
    }

    #[test]
    fn an_entity_spawned_on_a_wall_is_left_there_rather_than_moved() {
        // Spawning is what the caller asked for. A test that spawns at a known
        // cell has to find it at that cell.
        let mut map = Map::new(Size::new(6, 6), FLOOR);
        map.set_terrain(Point::new(2, 2), VOID);
        let mut state = GameState::new(map, 1);
        let uid = state.spawn(EntityType::Human, Point::new(2, 2));
        assert_eq!(
            state.entities().get(uid).unwrap().center_position(),
            Point::new(2, 2)
        );
    }
}
