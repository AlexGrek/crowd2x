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
//! GameState { map, entities, occupancy, log }
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
//! ## Two passes
//!
//! A tick is [`spawn_pass`] then [`process_pass`], and the division is the
//! whole point:
//!
//! | | writes the entity table | allocates | parallelisable |
//! | --- | --- | --- | --- |
//! | [`spawn_pass`] | **yes** — the only thing that does | yes | no |
//! | [`process_pass`] | **no** | no | that is what it is for |
//!
//! **The spawn pass** drains [`Input::commands`]. Spawning and despawning are
//! the only things that change *which* entities exist, and they all happen
//! here, first, so an entity spawned this tick thinks this tick.
//!
//! **The processing pass** advances every entity and adds or removes none. It
//! is handed a [`FrozenEntities`] — the table with its membership frozen,
//! offering no `insert` and no `remove` — so that is a guarantee the compiler
//! holds, not a rule to remember. Inside it, three steps:
//!
//! 1. **Think.** Read-only over the entities and the map. Every entity returns
//!    an [`Intent`] into a slot-indexed buffer. Nothing is mutated, so there
//!    is nothing to synchronise and this loop can be handed to a thread pool
//!    without changing its shape.
//! 2. **Move.** Single-threaded, slots in ascending order. Every intent is
//!    put to the world, which is allowed to refuse it, and what became of it
//!    lands in a second slot-indexed buffer as a [`MoveOutcome`].
//! 3. **React.** Every entity is handed its own outcome, once the whole crowd
//!    has moved. This is the second thinking round: an entity whose move was
//!    cancelled decides here what that means for its plan.
//!
//! **The intent buffer is the double buffer.** The `dev` skill asks for
//! read-from-A-write-to-B, and this is that, without cloning an entity: think
//! reads the entities and writes intents, move reads intents and writes the
//! entities. The two never touch the same memory in the same direction, which
//! is the property that makes the read phase safe to parallelise.
//!
//! Freezing the table is what makes that property survive contact with the
//! rest of the tick. The buffer is indexed by slot and sized once, in the
//! spawn pass; a `Vec` that grows mid-tick moves its contents and invalidates
//! every index and reference into it. Structural mutation is also the one
//! thing that genuinely cannot be parallelised — two threads moving different
//! entities never conflict, two threads pushing to one `Vec` always do. So the
//! pass that wants to run across threads is the pass that is not allowed to
//! change the table.
//!
//! ## Why moving is its own step, and why it is sequential
//!
//! Because **an entity does not get to decide where it ends up** — the world
//! does. [`Intent`] is a request; the move step is the one place it is
//! granted or refused, against the terrain ([`Map::is_passable`]) and then
//! against the crowd ([`Occupancy`]). Nobody walks through a wall and nobody
//! walks through anybody, and both facts are enforced in one loop rather than
//! trusted to every kind of entity that will ever exist.
//!
//! That loop is **strictly sequential, in slot order**, and it has to be: it
//! writes the occupancy layer, so two entities stepping into the same empty
//! cell in the same tick is precisely the race it exists to settle. The
//! earlier slot gets the cell, the later one is told who took it, and running
//! the same world twice settles it the same way. This is the one step of the
//! tick that is not parallelisable, and it is small — a cell test, a claim and
//! a position write.
//!
//! ## ...and why reacting is a separate round
//!
//! A collision is information, and it only exists once the move has been
//! tried. Answering it inside the move loop would mean an entity reacting to a
//! world that is halfway through the tick — the entities in later slots have
//! not moved yet, so the cell it is looking at may be about to empty. So the
//! crowd moves, and *then* the crowd reacts, entity by entity, each writing
//! only to itself. See [`GameEntity::react`].
//!
//! # Lock-free
//!
//! There is no `Mutex` here and there is not meant to be one. That is not
//! achieved with atomics — it is achieved by there being no shared mutable
//! state during the phase that runs concurrently. The one thing many threads
//! genuinely write at once is [`Log`], which is a bounded lock-free queue
//! precisely so it does not become the global lock that killed the earlier
//! prototype.

// Same reasoning as `src/map/`: this is the foundation for a game that is
// mostly not written yet, so a good deal of the surface — `Command::Despawn`,
// `elapsed`, `get_mut` — is reached only from tests until there is something
// that wants it. Trimming to today's single caller means re-adding it a piece
// at a time.
//
// The allow is not a licence to leave anything here. It hides *unused*, not
// *redundant*: `GameEntity::describe` and `Input::cursor` were both removed
// during review because nothing would ever have wanted them, not because
// nothing did yet.
#![allow(dead_code, unused_imports)]

pub mod entities;
pub mod entity;
pub mod kinds;
pub mod log;
pub mod occupancy;
pub mod path;
pub mod uid;

use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use rayon::prelude::*;

use crate::map::{Map, Point};

pub use entities::{Entities, FrozenEntities, Slot};
pub use entity::{cell_of, Body, GameEntity, Think};
pub use kinds::{Dog, Facing, Human};
pub use log::Log;
pub use occupancy::Occupancy;
pub use path::{find_path, Path, PathFinder};
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
    /// Be here at the end of the tick.
    ///
    /// A position and nothing else. Where the entity is *heading* used to ride
    /// along here, because think picks a goal and think may not write — but a
    /// route is chosen in the reaction round now, which may, so an intent is
    /// back to being one request about one tick.
    Move { to: (f32, f32) },
}

/// What became of an [`Intent`] once the world had its say.
///
/// The move step's answer to one entity, and the only thing
/// [`GameEntity::react`] is told about the tick it has just had. `Copy` and
/// slot-indexed for the same reasons [`Intent`] is: one per entity per tick,
/// in a buffer that is reused rather than reallocated.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MoveOutcome {
    /// Nothing was asked for, so nothing happened.
    #[default]
    Idle,
    /// It is where it wanted to be.
    Moved,
    /// **The move was cancelled** and the entity did not move at all — not
    /// part of the way, not along the obstacle. It is exactly where it started
    /// the tick.
    ///
    /// `by` names the entity in the way, and is `None` when the obstacle was
    /// the map itself: a wall, or the edge of it. Both are a refusal; which
    /// one it was is the difference between something that will never move and
    /// something that might, which is why the id is carried rather than
    /// flattened into a bool.
    Blocked { by: Option<Uid> },
}

impl MoveOutcome {
    pub fn is_blocked(self) -> bool {
        matches!(self, MoveOutcome::Blocked { .. })
    }

    /// The entity that got in the way, if one did.
    pub fn obstacle(self) -> Option<Uid> {
        match self {
            MoveOutcome::Blocked { by } => by,
            _ => None,
        }
    }
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
/// A struct rather than a `Vec<Command>` argument so that giving the
/// simulation something new to read — a pointer position, a paused flag —
/// does not change [`process_game_state`]'s signature and every call site with
/// it. It holds only commands today; a field nothing reads is ceremony, and
/// this one had a cursor in it until nothing turned out to want one.
#[derive(Clone, Default, Debug)]
pub struct Input {
    /// Applied in order, before anything thinks.
    pub commands: Vec<Command>,
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
    /// The dynamic half of passability: who is standing where. Kept in step by
    /// the spawn pass and by the move step, and by nothing else — see
    /// [`Occupancy`].
    occupancy: Occupancy,
    /// Written from any thread, drained by the renderer. See [`Log`].
    pub log: Log,
    /// Mints ids and seeds new entities. Seeded, so a run replays.
    rng: SmallRng,
    tick: u64,
    elapsed: f64,
    /// The back buffer, reused between ticks so the processing pass allocates
    /// nothing. Indexed by [`Slot`], and sized only by the spawn pass — a hole
    /// in the arena keeps an [`Intent::Idle`] that the move step skips.
    intents: Vec<Intent>,
    /// What the move step made of each intent, read by the reaction round.
    /// Indexed by [`Slot`] and sized alongside [`GameState::intents`].
    moves: Vec<MoveOutcome>,
}

impl GameState {
    /// An empty world on `map`.
    ///
    /// No entities: [`GameState::spawn`] is the only way one arrives, so a
    /// world is exactly what its caller asked for and nothing appears by
    /// default.
    pub fn new(map: Map, seed: u64) -> GameState {
        GameState {
            occupancy: Occupancy::new(map.size()),
            map,
            entities: Entities::new(),
            log: Log::new(),
            rng: SmallRng::seed_from_u64(seed),
            tick: 0,
            elapsed: 0.0,
            intents: Vec::new(),
            moves: Vec::new(),
        }
    }

    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    /// Who is standing where. Read-only from outside the tick: the two places
    /// allowed to write it are the spawn pass and the move step, which is what
    /// keeps it in step with the entity positions it describes.
    pub fn occupancy(&self) -> &Occupancy {
        &self.occupancy
    }

    /// What became of an entity's move on the last tick.
    ///
    /// `None` for an id nobody holds. The renderer has no use for this yet —
    /// it is how a test, a log or a future bump animation asks whether
    /// somebody actually got where they were going.
    pub fn last_move(&self, uid: Uid) -> Option<MoveOutcome> {
        let slot = self.entities.slot_of(uid)?;
        self.moves.get(slot as usize).copied()
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

        // The buffers are indexed by slot, so they have to cover the new one.
        if self.intents.len() <= slot as usize {
            self.intents.resize(slot as usize + 1, Intent::Idle);
            self.moves.resize(slot as usize + 1, MoveOutcome::Idle);
        }
        self.intents[slot as usize] = Intent::Idle;
        self.moves[slot as usize] = MoveOutcome::Idle;

        // Takes the cell if it is free, and leaves whoever is already there in
        // possession if it is not — an entity is placed where it was asked
        // for, so spawning is the one thing that can put two of them in one
        // cell. See the [`occupancy`] module docs; moving cannot.
        let _ = self.occupancy.claim(at, uid);

        self.log.push(format!("spawned {uid} at {},{}", at.x, at.y));
        uid
    }

    /// Remove an entity. Returns whether there was one.
    pub fn despawn(&mut self, uid: Uid) -> bool {
        match self.entities.remove(uid) {
            Some(gone) => {
                // Only if it was the registered occupant: an entity that
                // spawned on top of somebody else must not free their cell on
                // its way out.
                self.occupancy.release(gone.center_position(), uid);
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
            .field("occupancy", &self.occupancy)
            .field("log", &self.log)
            .finish()
    }
}

/// Advance the world by `dt` seconds: the spawn pass, then the processing
/// pass.
///
/// The one function. See the module docs for why it takes `&mut` rather than
/// returning a new state. The two passes are public in their own right, for a
/// caller that wants to spawn a world once and then run it — which is what the
/// QA harness's `tick` step does.
pub fn process_game_state(state: &mut GameState, dt: f32, input: &Input) {
    spawn_pass(state, input);
    process_pass(state, dt);
}

/// **The spawn pass: the only thing that changes which entities exist.**
///
/// Spawns and despawns, from [`Input::commands`]. It runs before the
/// processing pass, so something spawned this tick thinks this tick — watching
/// a new entity stand still for a frame reads as a bug in whatever spawned it.
///
/// This is also the only pass that may resize the entity table, and therefore
/// the only one that may allocate. [`GameState::spawn`] keeps the intent
/// buffer sized to the arena as it goes, which is what lets
/// [`process_pass`] index it without a bounds concern and without touching it.
pub fn spawn_pass(state: &mut GameState, input: &Input) {
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

/// **The processing pass: advances every entity, and adds or removes none.**
///
/// Three steps — [`think_step`], [`move_step`], [`react_step`] — over a table
/// whose membership is frozen: it is handed a [`FrozenEntities`], which has no
/// `insert` and no `remove`, so this is a guarantee the compiler holds rather
/// than a rule to remember.
///
/// Two things follow from the table's shape being fixed here, and both are the
/// reason for the split:
///
/// * **This pass allocates nothing.** Both slot buffers were sized by the
///   spawn pass and are only written through. A tick of a settled world
///   touches no allocator at all.
/// * **This pass is the one that becomes parallel.** Moving entities cannot
///   conflict; inserting into a shared `Vec` always does. Freezing membership
///   is what removes the only structural mutation from the phase that wants to
///   run across threads. Of the three steps, think and react are the ones that
///   parallelise; the move step is sequential on purpose, and the module docs
///   say why.
pub fn process_pass(state: &mut GameState, dt: f32) {
    state.tick += 1;
    state.elapsed += dt as f64;

    // Disjoint field borrows, so the borrow checker is what proves the steps
    // below do not overlap: think reads the world and writes `intents`, move
    // reads `intents` and writes the entities and `occupancy`, react reads
    // `moves` and writes the entities.
    let GameState {
        map,
        entities,
        occupancy,
        log,
        tick,
        intents,
        moves,
        ..
    } = state;

    let mut table = entities.freeze();

    // Sized by the spawn pass, which is the only thing that can change the
    // arena. If this ever trips, something grew the table outside that pass.
    debug_assert!(
        intents.len() >= table.capacity() && moves.len() >= table.capacity(),
        "the slot buffers ({}, {}) are shorter than the entity table ({})",
        intents.len(),
        moves.len(),
        table.capacity()
    );

    // The crowd as it ended the previous tick.
    think_step(
        &table,
        &Think {
            map,
            occupancy,
            log,
            dt,
            tick: *tick,
        },
        intents,
    );

    move_step(&mut table, map, occupancy, intents, moves);

    // ...and as it ends this one, which is the layer a detour is worth
    // planning against. Same struct, deliberately different moment.
    react_step(
        &mut table,
        &Think {
            map,
            occupancy,
            log,
            dt,
            tick: *tick,
        },
        moves,
    );
}

/// Live entities below which the rounds that could run in parallel do not
/// bother.
///
/// Handing eight entities to a thread pool costs more than thinking for them,
/// and every test in this crate and most of the QA suite runs worlds that
/// size. The answer is identical either side of the line — the parallel rounds
/// write one slot each and read nothing that changes — so this is a choice
/// about overhead and never about behaviour.
const PARALLEL_AT: usize = 64;

/// **Step 1: everybody decides.** Read-only, and run across threads.
///
/// Nothing is written but the intent buffer, and each entity writes only its
/// own slot, so the arena and the buffer are zipped index for index and handed
/// to rayon: two threads never touch the same `Intent` and nothing they read
/// changes while they read it. Keep it that way — an entity that wants to see
/// what another entity decided this tick is asking for the one thing this
/// shape cannot give.
fn think_step(table: &FrozenEntities<'_>, ctx: &Think<'_>, intents: &mut [Intent]) {
    let intents = &mut intents[..table.capacity()];
    if table.len() < PARALLEL_AT {
        for (slot, entity) in table.iter_slots() {
            intents[slot as usize] = entity.think(ctx);
        }
        return;
    }
    intents
        .par_iter_mut()
        .zip(table.par_iter_slots())
        .for_each(|(intent, entity)| {
            if let Some(entity) = entity {
                *intent = entity.think(ctx);
            }
        });
}

/// **Step 2: everybody moves, one at a time, and the world may say no.**
///
/// Sequential and in ascending slot order, because it writes the occupancy
/// layer: two entities heading for the same empty cell on the same tick is the
/// race this exists to settle, and it settles it the same way on every run.
///
/// An [`Intent::Move`] is granted only if the destination cell is passable
/// terrain *and* nobody else is standing in it. Otherwise the move is
/// **cancelled outright** — no partial step, no sliding along the wall — and
/// the entity is told what stopped it. Cancelling rather than trimming is what
/// makes the outcome meaningful: "you did not get there, and this is who was
/// in the way" is something an entity can act on, where "you got 40% of the
/// way" is not.
///
/// A step that stays inside the entity's own cell is the common case by a wide
/// margin — at a walking pace of two cells a second, a boundary is crossed
/// about once every thirty ticks — and it costs neither of the two checks: no
/// cell changes hands, so there is nothing to claim and nobody new to bump
/// into.
fn move_step(
    table: &mut FrozenEntities<'_>,
    map: &Map,
    occupancy: &mut Occupancy,
    intents: &[Intent],
    moves: &mut [MoveOutcome],
) {
    for (slot, entity) in table.iter_slots_mut() {
        let slot = slot as usize;
        let intent = intents[slot];
        let Intent::Move { to } = intent else {
            moves[slot] = MoveOutcome::Idle;
            continue;
        };

        let uid = entity.uid();
        let from = entity.center_position();
        let into = cell_of(to);

        moves[slot] = if into == from {
            // Still in its own cell: it already holds this one.
            entity.apply(&intent);
            MoveOutcome::Moved
        } else if !map.is_passable(into) {
            // The terrain first — a wall is cheaper to hit than a crowd, and
            // off the map is impassable, so this is the bounds check too.
            MoveOutcome::Blocked { by: None }
        } else if let Err(other) = occupancy.claim(into, uid) {
            // A refused claim writes nothing, so there is nothing to undo.
            MoveOutcome::Blocked { by: Some(other) }
        } else {
            // Claim first and release second: the other order would leave the
            // old cell open for a moment, which matters the day this runs
            // anywhere but here.
            occupancy.release(from, uid);
            entity.apply(&intent);
            MoveOutcome::Moved
        };
    }
}

/// **Step 3: everybody reacts, now that the whole crowd has moved.**
///
/// The second thinking round. Each entity is handed its own
/// [`MoveOutcome`] and revises its plan — for a wanderer, a cancelled move
/// means the cell it was heading for is not worth heading for.
///
/// Every entity is asked, not only the ones that were blocked: "did I get
/// there" is as much an answer as "who stopped me", and a kind that wants to
/// count its successful steps should not have to be told about them by a
/// separate channel. The default [`GameEntity::react`] does nothing, so a kind
/// with no plan pays a call and no more.
///
/// Each entity writes only to itself, so this runs across threads the same way
/// think does — a `&mut` walk of the table rather than a second intent buffer
/// nobody would read, and rayon over disjoint slots above [`PARALLEL_AT`].
///
/// **This is where pathfinding happens**, and it is the round that most wants
/// the threads: a far route is a search over the map, one walker's search
/// shares nothing with another's, and a crowd that has all arrived at once
/// wants all of them planned at once. See [`kinds::Walker`].
fn react_step(table: &mut FrozenEntities<'_>, ctx: &Think<'_>, moves: &[MoveOutcome]) {
    let moves = &moves[..table.capacity()];
    if table.len() < PARALLEL_AT {
        for (slot, entity) in table.iter_slots_mut() {
            entity.react(ctx, moves[slot as usize]);
        }
        return;
    }
    table
        .par_iter_slots_mut()
        .zip(moves.par_iter())
        .for_each(|(entity, outcome)| {
            if let Some(entity) = entity {
                entity.react(ctx, *outcome);
            }
        });
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
    fn the_processing_pass_never_changes_who_exists() {
        // `FrozenEntities` makes this impossible to get wrong at compile time;
        // the test is here to say it is a property of the design and not an
        // accident of how the loops happen to be written today.
        let mut state = world();
        let mut input = Input::new();
        input.spawn(EntityType::Human, Point::new(4, 4));
        input.spawn(EntityType::Dog, Point::new(6, 6));
        spawn_pass(&mut state, &input);

        let before: Vec<Uid> = state.entities().uids().collect();
        let arena = state.entities().capacity();

        for _ in 0..500 {
            process_pass(&mut state, 1.0 / 64.0);
        }

        assert_eq!(state.entities().uids().collect::<Vec<_>>(), before);
        assert_eq!(state.entities().capacity(), arena, "the arena was resized");
    }

    #[test]
    fn a_spawn_pass_with_nothing_to_do_leaves_the_world_alone() {
        let mut state = world();
        state.spawn(EntityType::Human, Point::new(4, 4));
        let before = state.entities().uids().collect::<Vec<_>>();

        spawn_pass(&mut state, &Input::new());

        assert_eq!(state.entities().uids().collect::<Vec<_>>(), before);
        // The spawn pass is not a tick: it moves no clock.
        assert_eq!(state.tick(), 0);
    }

    #[test]
    fn the_two_passes_together_are_the_one_function() {
        let mut input = Input::new();
        input.spawn(EntityType::Dog, Point::new(5, 5));

        let mut split = world();
        spawn_pass(&mut split, &input);
        process_pass(&mut split, 1.0 / 64.0);

        let mut combined = world();
        process_game_state(&mut combined, 1.0 / 64.0, &input);

        let positions = |s: &GameState| -> Vec<(u64, (f32, f32))> {
            s.entities()
                .iter()
                .map(|e| (e.uid().raw(), e.position()))
                .collect()
        };
        assert_eq!(positions(&split), positions(&combined));
        assert_eq!(split.tick(), combined.tick());
    }

    #[test]
    fn commands_are_applied_before_anything_thinks() {
        let mut state = world();
        let mut input = Input::new();
        input.spawn(EntityType::Human, Point::new(5, 5));

        process_game_state(&mut state, 1.0 / 60.0, &input);

        // Spawned this tick, and thought this tick — a wanderer's first think
        // finds no plan yet (there has been no reaction round to make one) and
        // asks for nothing, so it stands exactly where it landed rather than
        // somewhere `spawn` never put it.
        assert_eq!(state.len(), 1);
        let uid = state.entities().uids().next().expect("spawned");
        assert_eq!(state.position_of(uid), Some((5.5, 5.5)));

        // The reaction round that same tick planned a route (or is about to,
        // if the first random candidate missed the map), so a following tick
        // is the one that actually walks it.
        for _ in 0..20 {
            process_game_state(&mut state, 1.0 / 60.0, &Input::new());
        }
        assert_ne!(state.position_of(uid), Some((5.5, 5.5)));
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
        // Loose because dt is an f32: sixty of 1/60 accumulate to within about
        // 1e-7 of a second, and tightening this would only be asserting that
        // binary floating point works differently than it does.
        assert!((state.elapsed() - 1.0).abs() < 1e-6, "{}", state.elapsed());
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
    fn determinism_survives_crossing_into_the_parallel_rounds() {
        // The test above stays under `PARALLEL_AT`, so it never proves think
        // and react running on rayon instead of a plain loop still agree with
        // themselves. This one spawns past the threshold on purpose.
        let build = || {
            let mut state = GameState::new(Map::new(Size::new(24, 24), FLOOR), 11);
            for i in 0..(PARALLEL_AT as i32 + 20) {
                state.spawn(EntityType::Human, Point::new(i % 24, (i / 24) % 24));
            }
            state
        };
        assert!(build().len() > PARALLEL_AT, "should actually cross the line");

        let (mut a, mut b) = (build(), build());
        run(&mut a, 300);
        run(&mut b, 300);

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

    /// A solid wall is solid at every timestep, not only the one the game
    /// happens to run at.
    ///
    /// The sibling test above checks nobody *ends* a tick inside a wall, which
    /// a long step passes trivially by stepping clean over it — it did, and 14
    /// of 40 entities reached the far side. This asks the question that
    /// actually matters: did anyone get across.
    #[test]
    fn a_wall_cannot_be_stepped_over_however_long_the_step_is() {
        for dt in [1.0 / 64.0, 0.25, 1.0, 10.0] {
            let mut map = Map::new(Size::new(20, 3), FLOOR);
            for y in 0..3 {
                map.set_terrain(Point::new(10, y), WALL);
            }
            let mut state = GameState::new(map, 1);
            for i in 0..40 {
                state.spawn(EntityType::Dog, Point::new(1 + i % 9, 1));
            }

            let input = Input::new();
            for _ in 0..300 {
                process_game_state(&mut state, dt, &input);
            }

            let escaped = state
                .entities()
                .iter()
                .filter(|e| e.center_position().x > 10)
                .count();
            assert_eq!(escaped, 0, "{escaped} entities crossed the wall at dt={dt}");
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
        // The tombstone rule, end to end: the slot buffers must stay lined up
        // with the arena across a removal, so nobody is moved on the strength
        // of a decision filed under somebody else's slot.
        //
        // The entity that goes is walled off from the four that stay, and it
        // has to be: entities collide now, so an entity leaving a room really
        // does change what the entities in it do next. What must not change is
        // anything about an entity it could never have touched.
        let world = || {
            let mut map = Map::new(Size::new(16, 16), FLOOR);
            for y in 0..16 {
                map.set_terrain(Point::new(8, y), WALL);
            }
            GameState::new(map, 11)
        };
        let (mut a, mut b) = (world(), world());
        let mut keep = Vec::new();
        // Slots 0, 1, 3 and 4 share the left room; slot 2 — the hole this
        // makes, in the middle of the arena — is alone on the far side.
        let places = [
            Point::new(2, 8),
            Point::new(3, 8),
            Point::new(12, 8),
            Point::new(5, 8),
            Point::new(6, 8),
        ];
        for state in [&mut a, &mut b] {
            let mut ids = Vec::new();
            for place in places {
                ids.push(state.spawn(EntityType::Human, place));
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
    fn a_move_into_an_occupied_cell_is_cancelled_and_names_the_occupant() {
        // A world two cells wide: the only place either of them can go is
        // where the other one is standing, so every step is a collision.
        let mut state = GameState::new(Map::new(Size::new(2, 1), FLOOR), 4);
        let a = state.spawn(EntityType::Human, Point::new(0, 0));
        let b = state.spawn(EntityType::Human, Point::new(1, 0));
        let (start_a, start_b) = (Point::new(0, 0), Point::new(1, 0));

        let mut named_the_obstacle = false;
        for tick in 0..300 {
            run(&mut state, 1);

            let cell = |uid| state.entities().get(uid).unwrap().center_position();
            assert_eq!(cell(a), start_a, "tick {tick}");
            assert_eq!(cell(b), start_b, "tick {tick}");

            // Blocked *by the other entity*, not by the wall the map does not
            // have: a collision has to say who, or a reaction cannot tell the
            // difference between somebody in the way and nowhere to go.
            if state.last_move(a) == Some(MoveOutcome::Blocked { by: Some(b) }) {
                named_the_obstacle = true;
            }
        }
        assert!(named_the_obstacle, "they never bumped into each other");
    }

    #[test]
    fn two_entities_stepping_into_one_empty_cell_are_settled_by_slot_order() {
        // Wandering produces a genuine tie only by luck, and the arbitration
        // has to be exact whether or not it is lucky — so the intents are put
        // in by hand and the move step is run on its own.
        let mut state = world();
        let contested = Point::new(5, 4);
        let left = state.spawn(EntityType::Human, Point::new(4, 4));
        let right = state.spawn(EntityType::Human, Point::new(6, 4));

        for (uid, to) in [(left, (5.0, 4.5)), (right, (5.99, 4.5))] {
            let slot = state.entities.slot_of(uid).expect("just spawned") as usize;
            state.intents[slot] = Intent::Move { to };
        }

        let GameState {
            map,
            entities,
            occupancy,
            intents,
            moves,
            ..
        } = &mut state;
        move_step(&mut entities.freeze(), map, occupancy, intents, moves);

        // The earlier slot gets the cell; the later one is told who took it
        // and has not moved at all.
        assert_eq!(state.last_move(left), Some(MoveOutcome::Moved));
        assert_eq!(
            state.last_move(right),
            Some(MoveOutcome::Blocked { by: Some(left) })
        );
        assert_eq!(state.occupancy().occupant(contested), Some(left));
        assert_eq!(state.position_of(right), Some((6.5, 4.5)));
    }

    #[test]
    fn moving_never_puts_two_entities_in_one_cell() {
        // The headline property, over a crowd dense enough to keep bumping
        // into itself: twelve of them in a room of thirty-six cells.
        let mut state = GameState::new(Map::new(Size::new(6, 6), FLOOR), 13);
        for i in 0..12 {
            state.spawn(EntityType::Human, Point::new(i % 6, i / 6));
        }

        for tick in 0..1000 {
            run(&mut state, 1);
            let mut taken = std::collections::HashSet::new();
            for entity in state.entities().iter() {
                assert!(
                    taken.insert(entity.center_position()),
                    "{} shares {:?} on tick {tick}",
                    entity.uid(),
                    entity.center_position()
                );
            }
        }
    }

    #[test]
    fn the_occupancy_layer_follows_an_entity_as_it_walks() {
        // A cell claimed and not given back is a hole in the map that nothing
        // can ever walk through again, and nothing would say so.
        let mut state = world();
        let uid = state.spawn(EntityType::Human, Point::new(4, 4));

        for tick in 0..500 {
            run(&mut state, 1);
            let cell = state.entities().get(uid).expect("alive").center_position();
            assert_eq!(state.occupancy().occupant(cell), Some(uid), "tick {tick}");
            assert_eq!(
                state.occupancy().count_occupied(),
                1,
                "a cell was left claimed behind it on tick {tick}"
            );
        }
    }

    #[test]
    fn despawning_gives_back_the_cell_the_entity_was_standing_in() {
        let mut state = world();
        let uid = state.spawn(EntityType::Human, Point::new(4, 4));
        run(&mut state, 200);
        let cell = state.entities().get(uid).expect("alive").center_position();
        assert_eq!(state.occupancy().occupant(cell), Some(uid));

        state.despawn(uid);

        assert_eq!(state.occupancy().occupant(cell), None);
        assert_eq!(state.occupancy().count_occupied(), 0);
    }

    #[test]
    fn spawning_on_top_of_somebody_leaves_them_in_possession() {
        // Spawning puts an entity where it was asked for, occupied or not, so
        // it is the one thing that can put two of them in one cell — and the
        // newcomer must not take the cell off the entity already holding it,
        // in either direction. See the `occupancy` module docs.
        let mut state = world();
        let cell = Point::new(4, 4);
        let first = state.spawn(EntityType::Human, cell);
        let second = state.spawn(EntityType::Human, cell);

        assert_eq!(state.len(), 2);
        assert_eq!(state.occupancy().occupant(cell), Some(first));
        assert_eq!(
            state.entities().get(second).expect("spawned").center_position(),
            cell
        );

        state.despawn(second);
        assert_eq!(state.occupancy().occupant(cell), Some(first));
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
    fn a_tick_allocates_no_slot_buffer_after_the_first() {
        let mut state = world();
        for i in 0..10 {
            state.spawn(EntityType::Human, Point::new(1, 1 + i));
        }
        run(&mut state, 1);
        let (intents, moves) = (state.intents.capacity(), state.moves.capacity());
        run(&mut state, 500);
        assert_eq!(state.intents.capacity(), intents);
        assert_eq!(state.moves.capacity(), moves);
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

