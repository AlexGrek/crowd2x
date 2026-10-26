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
//! holds, not a rule to remember. Inside it, four steps:
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
//!    cancelled decides here what that means for its plan. It may write only
//!    to itself, and hands back an [`Effect`] for anything it wants to ask of
//!    the world beyond itself, into a third slot-indexed buffer.
//! 4. **World.** Single-threaded, slots in ascending order, like move: every
//!    [`Effect`] is applied — a fridge's door, a door's leaf, a piece off a
//!    heap, a pack emptied into a bin — and then the world's own state that
//!    is not any one entity's moves on by the tick's worth of time. See
//!    [`world_step`].
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

pub mod background;
pub mod bins;
pub mod biology;
pub mod brain;
pub(crate) mod catalogue;
pub mod census;
pub mod clock;
pub mod clothing;
pub mod corpses;
pub mod deathdogs;
pub mod door;
pub mod dirt;
mod soiling;
pub mod economy;
pub mod effect;
pub mod electrical;
pub mod entities;
pub mod entity;
pub(crate) mod facilities;
pub mod feature;
pub mod fields;
pub mod flowers;
pub mod fridge;
pub mod homes;
pub mod hospital;
pub mod identity;
pub mod inventory;
pub mod item;
pub mod job;
pub mod kinds;
pub mod light_switch;
pub mod log;
pub mod occupancy;
pub mod panics;
pub mod path;
pub mod pets;
pub mod playzone;
pub mod property;
pub mod refuge;
pub mod relationships;
pub(crate) mod rng;
mod saving;
pub mod statistics;
pub mod sun;
pub mod talents;
#[cfg(test)]
pub(crate) mod testing;
pub mod uid;
pub mod walker;
pub mod wounds;

use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use rayon::prelude::*;

use crate::map::utilities::Supply;
use crate::map::{Map, ObjectLayer, Point};

pub use bins::Bins;
pub use biology::{Biology, ProcessId, Stats};
pub use brain::{Brain, GoalId};
pub use census::Census;
pub use clock::Clock;
pub use corpses::Corpses;
pub use sun::Sun;
pub use door::Doors;
pub use effect::Effect;
pub use entities::{Entities, FrozenEntities, Slot};
pub use entity::{cell_of, Body, GameEntity, Think};
pub use feature::{FeatureKind, Features};
pub use fields::Fields;
pub use fridge::Fridges;
pub use homes::Homes;
pub use inventory::{Capacity, Inventory};
pub use item::ItemKind;
pub use job::{Job, JobKind, Workplaces};
pub use kinds::{Deathdog, Dog, Facing, Human};
pub use light_switch::LightSwitches;
pub use log::Log;
pub use occupancy::Occupancy;
pub use path::{Path, PathFinder};
pub use playzone::Playzones;
#[cfg(test)]
pub use path::find_path;
pub use property::{Properties, PropertyId};
pub use talents::{Talent, Talents};
pub use uid::{EntityType, Uid};
pub use saving::RestoredEntity;
pub use statistics::{Statistics, Tally};
pub use walker::Walker;

use facilities::Facilities;

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
    /// Replace ground or carried contamination; invalid fractions are refused.
    SetDirt { cell: Point, value: dirt::Dirt },
    SetUnitDirt { uid: Uid, value: dirt::Dirt },
    /// Change what a person wears, independently of their identity.
    SetClothing { uid: Uid, clothing: clothing::Clothing },
    Spawn { kind: EntityType, at: Point },
    Despawn(Uid),
    /// Hold an entity still, or let it go again.
    ///
    /// Not a structural change, but a command all the same: it comes from
    /// outside the world — somebody looking at an entity through the unit
    /// panel — and it has to reach the entity by id, which is the shape every
    /// command has. Applied in the spawn pass with the others, so a freeze
    /// asked for this tick is in force before anything thinks.
    Freeze { uid: Uid, frozen: bool },
    /// Switch one of an entity's biological processes on or off — see
    /// [`biology`] for what off means. From outside the world for the same
    /// reasons a freeze is, and applied at the same point in the tick.
    SetProcess { uid: Uid, process: ProcessId, running: bool },
    /// Change one lasting aptitude explicitly; time alone does not move it.
    /// Values are clamped to 0..=100 and applied before processing begins.
    SetTalent { uid: Uid, talent: Talent, value: i32 },
    /// Put an item into an entity's [`Inventory`], if it will fit.
    ///
    /// From outside the world like the two above, and the only way anything
    /// gets stowed today: the brain takes things into a *hand* and consumes
    /// them from there, and no goal yet has a reason to pack something away.
    /// So this is what a debug harness or a QA script uses to put a unit in a
    /// state the game cannot reach on its own — the same footing
    /// [`Command::SetProcess`] is on, and it will stop being the only route
    /// the moment a goal wants to fetch something.
    GiveItem { uid: Uid, item: ItemKind },
    /// Put an item in an entity's **hand**, or empty it with `None`.
    ///
    /// The other half of [`Command::GiveItem`], for the slot the brain really
    /// uses. A hand is only ever full for the length of a meal, so a QA script
    /// that wants to look at one — or a debug harness that wants to know what
    /// a unit carrying something looks like — cannot wait for the brain to
    /// oblige. Like [`Inventory::set_hand`] it refuses nothing, and what was in
    /// the hand is gone.
    Hold { uid: Uid, item: Option<ItemKind> },
    /// Switch the distribution box in `at` on or off — a player at its
    /// panel. Off, it feeds no wiring, and everything on that wiring stops
    /// working: see [`GameState::switch_box`].
    SwitchBox { at: Point, on: bool },
    /// Flip the light switch in `at` on or off — a player's click on it. It
    /// switches the lamps of its room, and every other switch in that room
    /// with it: see [`GameState::switch_light`].
    SwitchLight { at: Point, on: bool },
    /// Tell a unit to salvage the heap of scrap at `at`: walk to it and work
    /// it a piece at a time, until it is empty or the unit can carry no more
    /// ([`GameState::order_salvage`]). An errand: a scrap collector goes to
    /// the heaps of its own accord (`brain::goals::CollectGoal`).
    Salvage { uid: Uid, at: Point },
    /// Give a unit a job, at the free workplace nearest where it stands —
    /// a desk if there is one, a scrap bin if not ([`GameState::hire`]). The
    /// other way a job is handed out — the first is arriving to find one
    /// free.
    Hire(Uid),
    /// Take a unit's job away and give its workplace back
    /// ([`GameState::dismiss`]).
    Dismiss(Uid),
    /// Set one of the economy's numbers ([`economy::Tariff`]) to `value`:
    /// cents for money, percent for the tax, minutes for an account — the
    /// player at the economy menu. Kept in bounds and logged
    /// ([`GameState::set_tariff`]).
    SetTariff { tariff: economy::Tariff, value: i64 },
    /// Set one of the rules relationships are run by
    /// ([`relationships::Rule`]) to `value` — a QA script's `relationships`
    /// step. Kept in bounds and logged ([`GameState::set_relationship_rule`]).
    SetRelationshipRule { rule: relationships::Rule, value: f64 },
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
    pub fn set_dirt(&mut self, cell: Point, value: dirt::Dirt) -> &mut Input {
        self.commands.push(Command::SetDirt { cell, value }); self
    }

    pub fn set_unit_dirt(&mut self, uid: Uid, value: dirt::Dirt) -> &mut Input {
        self.commands.push(Command::SetUnitDirt { uid, value }); self
    }

    pub fn set_clothing(&mut self, uid: Uid, clothing: clothing::Clothing) -> &mut Input {
        self.commands.push(Command::SetClothing { uid, clothing });
        self
    }
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

    pub fn switch_box(&mut self, at: Point, on: bool) -> &mut Input {
        self.commands.push(Command::SwitchBox { at, on });
        self
    }

    pub fn switch_light(&mut self, at: Point, on: bool) -> &mut Input {
        self.commands.push(Command::SwitchLight { at, on });
        self
    }

    pub fn freeze(&mut self, uid: Uid, frozen: bool) -> &mut Input {
        self.commands.push(Command::Freeze { uid, frozen });
        self
    }

    pub fn set_process(&mut self, uid: Uid, process: ProcessId, running: bool) -> &mut Input {
        self.commands.push(Command::SetProcess { uid, process, running });
        self
    }

    pub fn set_talent(&mut self, uid: Uid, talent: Talent, value: i32) -> &mut Input {
        self.commands.push(Command::SetTalent { uid, talent, value });
        self
    }

    pub fn give_item(&mut self, uid: Uid, item: ItemKind) -> &mut Input {
        self.commands.push(Command::GiveItem { uid, item });
        self
    }

    pub fn salvage(&mut self, uid: Uid, at: Point) -> &mut Input {
        self.commands.push(Command::Salvage { uid, at });
        self
    }

    pub fn hold(&mut self, uid: Uid, item: Option<ItemKind>) -> &mut Input {
        self.commands.push(Command::Hold { uid, item });
        self
    }

    pub fn hire(&mut self, uid: Uid) -> &mut Input {
        self.commands.push(Command::Hire(uid));
        self
    }

    pub fn dismiss(&mut self, uid: Uid) -> &mut Input {
        self.commands.push(Command::Dismiss(uid));
        self
    }

    pub fn set_tariff(&mut self, tariff: economy::Tariff, value: i64) -> &mut Input {
        self.commands.push(Command::SetTariff { tariff, value });
        self
    }

    pub fn set_relationship_rule(&mut self, rule: relationships::Rule, value: f64) -> &mut Input {
        self.commands.push(Command::SetRelationshipRule { rule, value });
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

/// How many chunks each of [`GameState::follow_the_crowd`]'s sweeps looks at
/// per spawn pass: once a frame, a few thousand chunks a second, so even a
/// crowd that has been everywhere is swept through in a second or two, and
/// no pass pays for more than this.
pub const RECLAIM_EACH_PASS: usize = 64;

/// How near somebody, in chunks, a chunk is kept. One more than the ring
/// loaded round each occupied chunk, so walking to and fro over a chunk's
/// edge does not drop and load the same chunks over and over.
pub const KEEP_CHUNKS: i32 = 2;

/// Where each of [`GameState::follow_the_crowd`]'s chunk sweeps got to.
#[derive(Clone, Copy, Debug)]
struct Reclaim {
    ground: usize,
    occupancy: usize,
    dirt: usize,
    census: usize,
    /// Off only in a test, to compare with a world that keeps everything.
    on: bool,
}

impl Default for Reclaim {
    fn default() -> Self {
        Reclaim { ground: 0, occupancy: 0, dirt: 0, census: 0, on: true }
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
    /// Who is in which chunk, everybody: kept in step by the spawn pass and
    /// the move step, like `occupancy`. See [`census`].
    census: Census,
    /// Where each of [`GameState::follow_the_crowd`]'s sweeps got to, and
    /// whether it sweeps at all.
    reclaim: Reclaim,
    /// The chunks somebody is in, gathered by the spawn pass and kept for
    /// the capacity.
    crowded: Vec<crate::map::ChunkPos>,
    /// Written from any thread, drained by the renderer. See [`Log`].
    pub log: Log,
    /// The world's parts beside the map and the crowd: the features and
    /// what is in them, the doors, whose rooms, beds and workplaces are
    /// whose, the dead. See [`facilities`].
    facilities: Facilities,
    /// What the map's wiring and pipes reach ([`crate::map::utilities`]),
    /// with [`GameState::boxes_off`] switched off. Derived from `map` like
    /// `features`, which is built from it — and derived again, with the
    /// features and the fridges' power, whenever a box is switched.
    supply: Supply,
    /// The distribution boxes switched off, sorted. Simulation state, not
    /// the map's: a map is saved with every box on.
    boxes_off: Vec<Point>,
    /// The boxes a street light controller stands on, sorted
    /// ([`crate::map::utilities::controlled_boxes`]). Derived from `map`
    /// once: on while it is dark, off while it is light.
    controlled: Vec<Point>,
    /// Whether the controllers' photocells last saw night — the state the
    /// supply was flooded for. Followed by [`GameState::follow_daylight`].
    street_lights_on: bool,
    /// The supply and features with the street lights the *other* way,
    /// flooded beside the live ones by [`GameState::refresh_supply`], so dusk
    /// and dawn are a swap inside a tick rather than a flood — which is what
    /// lets them happen on the exact tick the sun says, however the ticks are
    /// grouped into frames. `None` on a map with no controller.
    other_light: Option<(Supply, Features)>,
    /// Every light switch, the room it switches and whether it is on. The
    /// rooms are derived from `map` once; which are off is simulation state,
    /// all on when a world begins. See [`light_switch`].
    light_switches: LightSwitches,
    /// Bumped every time `supply` changes, so a reader — the lighting, the
    /// x-ray — knows to look again without comparing the whole thing.
    supply_generation: u64,
    /// Mints ids and seeds new entities. Seeded, so a run replays; and a
    /// named generator, so a save can keep its state ([`rng::WorldRng`]).
    rng: rng::WorldRng,
    tick: u64,
    elapsed: f64,
    /// The back buffer, reused between ticks so the processing pass allocates
    /// nothing. Indexed by [`Slot`], and sized only by the spawn pass — a hole
    /// in the arena keeps an [`Intent::Idle`] that the move step skips.
    intents: Vec<Intent>,
    /// What the move step made of each intent, read by the reaction round.
    /// Indexed by [`Slot`] and sized alongside [`GameState::intents`].
    moves: Vec<MoveOutcome>,
    /// What each entity asked of the world beyond itself, written by react
    /// and drained by [`world_step`] at the end of the tick. Indexed by
    /// [`Slot`] and sized alongside [`GameState::intents`].
    effects: Vec<Effect>,
    /// The chat each entity is in, as the think step found it — gathered into
    /// [`relationships::Chats`] before react reads it. Indexed by [`Slot`]
    /// and sized alongside [`GameState::intents`]; overwritten every tick, so
    /// never saved.
    chatters: Vec<Option<relationships::Chatter>>,
    /// Who is making small talk, slot for slot, as the think step found
    /// them: gathered into [`relationships::Smalltalks`] like `chatters`.
    smalltalkers: Vec<Option<relationships::Smalltalker>>,
    /// Who is bonded to whom, slot for slot, as the think step found them:
    /// gathered into [`pets::Bonds`] like `chatters`.
    bonders: Vec<Option<pets::Bond>>,
    /// Cells the far route searches expanded last tick, all of them — what
    /// the route budget bounds ([`react_step`]).
    route_work: usize,
}

impl GameState {
    /// An empty world on `map`.
    ///
    /// No entities: [`GameState::spawn`] is the only way one arrives, so a
    /// world is exactly what its caller asked for and nothing appears by
    /// default.
    pub fn new(mut map: Map, seed: u64) -> GameState {
        // The wasteland a route about the site may swing out into, in memory
        // before anybody asks: what reachability is worked out over, and the
        // window of every search that starts near the site. Further out is
        // loaded as the crowd gets there (`follow_the_crowd_out`).
        map.load_chunks(map.navigation_area());
        let _ = map.navigation();
        let controlled = crate::map::utilities::controlled_boxes(&map);
        let street_lights_on = Clock::after_watching(0.0).sun().is_dark();
        // Every box and every light switch on: only the controllers' boxes
        // can be off yet.
        let supply = flood_supply(&map, &[], &controlled, street_lights_on, std::iter::empty());
        let facilities = Facilities::new(&map, &supply);
        let other_light = (!controlled.is_empty()).then(|| {
            let other = flood_supply(&map, &[], &controlled, !street_lights_on, std::iter::empty());
            let features = Features::with_properties(&map, &other, &facilities.properties);
            (other, features)
        });
        GameState {
            occupancy: Occupancy::new(map.size()),
            census: Census::new(),
            reclaim: Reclaim::default(),
            crowded: Vec::new(),
            facilities,
            supply,
            boxes_off: Vec::new(),
            controlled,
            street_lights_on,
            other_light,
            // All on, so the supply above is already the one they want.
            light_switches: LightSwitches::from_map(&map),
            supply_generation: 0,
            map,
            entities: Entities::new(),
            log: Log::new(),
            rng: rng::WorldRng::seed_from_u64(seed),
            tick: 0,
            elapsed: 0.0,
            intents: Vec::new(),
            moves: Vec::new(),
            effects: Vec::new(),
            chatters: Vec::new(),
            smalltalkers: Vec::new(),
            bonders: Vec::new(),
            route_work: 0,
        }
    }

    /// Cells the far route searches expanded last tick, the whole crowd's
    /// together: never more than the route budget allows
    /// ([`ROUTE_BUDGET`], [`MIN_BLOCK_BUDGET`] a block).
    pub fn route_work(&self) -> usize {
        self.route_work
    }

    pub fn dirt(&self) -> &dirt::DirtLayer { &self.facilities.dirt }

    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    /// Who is in which chunk. See [`census`].
    pub fn census(&self) -> &Census {
        &self.census
    }

    /// Keep in memory what the crowd needs round it, and let go of what it
    /// has left behind.
    ///
    /// For every chunk somebody is in ([`Census::occupied`], not a walk of
    /// the crowd), the chunk and the eight round it: their census lists, so
    /// the move step files a unit crossing into one without allocating; and
    /// on an endless map their ground and the occupancy layer's cells for
    /// whatever of them is off the site, so the processing pass reads the
    /// ground from memory and claims a cell without allocating.
    ///
    /// Then a bounded sweep of each of the three — [`RECLAIM_EACH_PASS`]
    /// chunks, so the cost of a pass does not grow with how far the crowd
    /// has been — drops the chunks with nobody within [`KEEP_CHUNKS`]: the
    /// wasteland's ground (but never the navigation area round the site,
    /// which every search near it reads), the occupancy layer's empty
    /// chunks, and the census's empty lists. Keeping a ring wider than the
    /// one loaded means somebody walking to and fro over a chunk's edge does
    /// not load and drop the same chunks over and over.
    ///
    /// **None of it changes anything that happens.** A chunk of ground not
    /// loaded reads the same as one that is, only slower
    /// ([`crate::map::chunk`]); a chunk of occupancy with nobody in it reads
    /// the same as none, and one with anybody in it is never dropped. So this
    /// may lag the crowd by however many ticks a frame runs, and what is in
    /// memory when is never part of the outcome — a unit a long way from
    /// anything, that nobody is watching, is simulated exactly as one in
    /// view. Spawn-pass work, since it allocates.
    fn follow_the_crowd(&mut self) {
        let mut crowded = std::mem::take(&mut self.crowded);
        crowded.clear();
        crowded.extend(self.census.occupied());
        let endless = self.map.is_endless();
        for &pos in &crowded {
            self.census.prepare(pos);
            if endless {
                for near in pos.around(1) {
                    self.map.load_chunk(near);
                }
                self.occupancy.prepare(pos);
                self.facilities.dirt.prepare(pos);
            }
        }
        self.crowded = crowded;

        if self.reclaim.on {
            let GameState { map, occupancy, census, facilities, reclaim, .. } = self;
            let pinned = map.navigation_area();
            map.sweep_chunks(&mut reclaim.ground, RECLAIM_EACH_PASS, |pos| {
                pos.bounds().overlaps(pinned) || census.anyone_within(pos, KEEP_CHUNKS)
            });
            occupancy.sweep(&mut reclaim.occupancy, RECLAIM_EACH_PASS, |pos| {
                census.anyone_within(pos, KEEP_CHUNKS)
            });
            facilities.dirt.sweep(&mut reclaim.dirt, RECLAIM_EACH_PASS, |pos| census.anyone_within(pos, KEEP_CHUNKS));
            census.sweep(&mut reclaim.census, RECLAIM_EACH_PASS, KEEP_CHUNKS);
        }
    }

    /// Stop dropping chunks behind the crowd, so a test can compare a world
    /// that reclaims with one that keeps everything.
    #[cfg(test)]
    pub(crate) fn keep_every_chunk(&mut self) {
        self.reclaim.on = false;
    }

    /// An entity to set up by hand, for a test in another module.
    #[cfg(test)]
    pub(crate) fn entity_mut(&mut self, uid: Uid) -> Option<&mut (dyn GameEntity + 'static)> {
        self.entities.get_mut(uid)
    }

    /// How many chunks the occupancy layer holds off the site.
    pub fn wild_chunks(&self) -> usize {
        self.occupancy.wild_chunks()
    }

    /// What the map's props are for, and where. See [`feature`].
    pub fn features(&self) -> &Features {
        &self.facilities.features
    }

    /// Every fridge's door and temperature. See [`fridge`].
    /// What has power and what has a drain: what the features were indexed
    /// from, and what lights a lamp.
    pub fn supply(&self) -> &Supply {
        &self.supply
    }

    /// Which [`GameState::supply`] this is: bumped on every change to it.
    pub fn supply_generation(&self) -> u64 {
        self.supply_generation
    }

    /// Whether there is a distribution box in `at` at all.
    pub fn has_box(&self, at: Point) -> bool {
        self.map.grid(crate::map::GridLayer::Power, at) == crate::map::grid::power::BOX
    }

    /// Distribution box faults, read by brains and the renderer.
    pub fn electrical(&self) -> &electrical::Electrical {
        &self.facilities.electrical
    }

    pub fn box_is_damaged(&self, at: Point) -> bool {
        self.facilities.electrical.is_damaged(at)
    }

    /// The switch position, independently of damage. `false` with no box.
    pub fn box_is_on(&self, at: Point) -> bool {
        self.has_box(at) && self.boxes_off.binary_search(&at).is_err()
    }

    /// Switch the box in `at` on or off, and work out again what has power:
    /// the supply, the features a brain can go to — a fridge on a dead box
    /// is not one — and every fridge's compressor. Returns `false`, changing
    /// nothing, when there is no box there.
    ///
    /// Re-floods the whole map and rebuilds the feature index, which
    /// allocates: it is applied in the spawn pass (through
    /// [`Command::SwitchBox`]), the one pass allowed to, and it is a
    /// player's click, not something a tick does.
    pub fn switch_box(&mut self, at: Point, on: bool) -> bool {
        if !self.has_box(at) {
            return false;
        }
        if self.box_is_damaged(at) {
            self.log.important(format!("the distribution box at {}, {} is damaged: waiting for an electrician", at.x, at.y));
            return true;
        }
        match (self.boxes_off.binary_search(&at), on) {
            (Ok(index), true) => {
                self.boxes_off.remove(index);
            }
            (Err(index), false) => self.boxes_off.insert(index, at),
            // Already the way it was asked to be.
            _ => return true,
        }
        if self.rng.random_bool(electrical::BOX_FAILURE_CHANCE) {
            self.facilities.electrical.damage(at);
            self.log.important(format!("the distribution box at {}, {} broke while switching {}: damaged", at.x, at.y, if on { "on" } else { "off" }));
        }
        self.refresh_supply();
        self.log.push(format!(
            "the distribution box at {}, {} was switched {}",
            at.x,
            at.y,
            if on { "on" } else { "off" }
        ));
        true
    }

    /// Whether a street light controller stands on the box in `at`.
    pub fn is_controlled(&self, at: Point) -> bool {
        self.controlled.binary_search(&at).is_ok()
    }

    /// Whether the street light controllers have their boxes on: it is dark.
    pub fn street_lights_on(&self) -> bool {
        self.street_lights_on
    }

    /// The street light controllers look at the sky at `clock`: if it has
    /// gone dark since they last looked, every controlled box closes, and if
    /// it has grown light, every one opens. A player's switch still wins: a
    /// box switched off stays off at night.
    ///
    /// Called at the top of every [`process_pass`], so the lights change on
    /// the first tick the sun says they should — the same tick on every run,
    /// at any game speed, however the ticks fall into frames. It floods
    /// nothing and allocates nothing but its log line: the supply the other
    /// way was flooded in advance ([`GameState::other_light`]), and this
    /// swaps it in. Nothing at all happens on a tick the photocells see what
    /// they saw before, which is every tick but two a world day, or on a map
    /// with no controller.
    fn follow_daylight(&mut self, clock: Clock) {
        let dark = clock.sun().is_dark();
        if dark == self.street_lights_on {
            return;
        }
        let Some((supply, features)) = self.other_light.as_mut() else { return };
        std::mem::swap(&mut self.supply, supply);
        std::mem::swap(&mut self.facilities.features, features);
        self.street_lights_on = dark;
        self.facilities.fridges.set_power(&self.supply);
        self.supply_generation += 1;
        self.log.push(format!(
            "the street lights were switched {} at {}",
            if dark { "on" } else { "off" },
            clock.time()
        ));
    }

    /// Every light switch, the room each switches and whether it is on.
    /// See [`light_switch`].
    pub fn light_switches(&self) -> &LightSwitches {
        &self.light_switches
    }

    /// Whether there is a light switch in `at`.
    pub fn has_light_switch(&self, at: Point) -> bool {
        self.light_switches.has(at)
    }

    /// Whether the light switch in `at` is on. `false` where there is none.
    pub fn light_is_on(&self, at: Point) -> bool {
        self.light_switches.is_on(at)
    }

    /// Flip the light switch in `at` on or off: the lamps of its room — and
    /// only the lamps — lose or get back their power, and every other switch
    /// in the room reads the same. Returns `false`, changing nothing, when
    /// there is no switch there.
    ///
    /// Re-floods the supply like [`GameState::switch_box`], for the same
    /// reason and on the same terms: applied in the spawn pass (through
    /// [`Command::SwitchLight`]), and a click, not something a tick does.
    pub fn switch_light(&mut self, at: Point, on: bool) -> bool {
        match self.light_switches.switch(at, on) {
            None => false,
            Some(false) => true,
            Some(true) => {
                self.refresh_supply();
                self.log.push(format!(
                    "the light switch at {}, {} was switched {}",
                    at.x,
                    at.y,
                    if on { "on" } else { "off" }
                ));
                true
            }
        }
    }

    /// Flood the networks again with every box that is off — switched off by
    /// hand, or opened by its controller in daylight — and every room whose
    /// light switch is off, and rebuild what follows from them: the features
    /// a brain can go to (a fridge on a dead box is not one) and every
    /// fridge's compressor. On a map with street light controllers the
    /// supply with the street lights the other way is flooded too, for
    /// [`GameState::follow_daylight`] to swap in at dusk or dawn.
    fn refresh_supply(&mut self) {
        let mut disconnected = self.boxes_off.clone();
        disconnected.extend_from_slice(self.electrical().damaged());
        disconnected.sort_unstable();
        disconnected.dedup();
        let flood = |lights_on: bool| {
            flood_supply(&self.map, &disconnected, &self.controlled, lights_on, self.light_switches.dark_cells())
        };
        self.supply = flood(self.street_lights_on);
        self.facilities.features = Features::with_properties(&self.map, &self.supply, &self.facilities.properties);
        if !self.controlled.is_empty() {
            let other = flood(!self.street_lights_on);
            let features = Features::with_properties(&self.map, &other, &self.facilities.properties);
            self.other_light = Some((other, features));
        }
        self.facilities.fridges.set_power(&self.supply);
        self.supply_generation += 1;
        self.facilities.electrical.supply_dirty = false;
    }

    pub fn fridges(&self) -> &Fridges {
        &self.facilities.fridges
    }

    /// Who lies dead where. See [`corpses`].
    pub fn corpses(&self) -> &Corpses {
        &self.facilities.corpses
    }

    /// What every scrap bin holds. See [`bins`].
    pub fn bins(&self) -> &Bins {
        &self.facilities.bins
    }

    /// Which playzones are being run, and so which arcade cabinets work. See
    /// [`playzone`].
    pub fn playzones(&self) -> &Playzones {
        &self.facilities.playzones
    }

    /// Every crop in the fields. See [`fields`].
    pub fn flowers(&self) -> &flowers::Flowers {
        &self.facilities.flowers
    }

    pub fn fields(&self) -> &Fields {
        &self.facilities.fields
    }

    /// The district's money. See [`economy`].
    pub fn economy(&self) -> &economy::Economy {
        &self.facilities.economy
    }

    /// Everybody's family and job: what the floors of their relationships
    /// are made of ([`relationships::Society`]), and the rules.
    pub fn society(&self) -> &relationships::Society {
        &self.facilities.society
    }

    /// Run relationships by `rules` — a new game's, from
    /// `relationships.yaml`.
    pub fn set_relationship_rules(&mut self, rules: relationships::Rules) {
        self.facilities.society.set_rules(rules);
    }

    /// Set one rule, kept in bounds, and say so
    /// ([`Command::SetRelationshipRule`]). What it was set to.
    pub fn set_relationship_rule(&mut self, rule: relationships::Rule, value: f64) -> f32 {
        let set = self.facilities.society.change_rule(rule, value);
        self.log.important(format!("relationships: {} is {set}", rule.name()));
        set
    }

    /// Everybody `uid` has a relationship with — family, colleagues, and
    /// whoever it has chatted with — best first ([`relationships::acquaintances`]).
    /// Empty for a kind that knows nobody. Allocates: for the relationship
    /// tab, about the one unit selected.
    pub fn acquaintances(&self, uid: Uid) -> Vec<relationships::Acquaintance> {
        match self.entities.get(uid).and_then(|entity| entity.relations()) {
            Some(relations) => relationships::acquaintances(uid, relations, &self.facilities.society),
            None => Vec::new(),
        }
    }

    /// Run the economy by `tariffs` from now on: what a new game reads out of
    /// `economy.yaml` before anybody arrives, since an arrival's grant and
    /// cash are the tariffs' of the moment.
    pub fn set_tariffs(&mut self, tariffs: economy::Tariffs) {
        self.facilities.economy.set_tariffs(tariffs);
    }

    /// Set one tariff, kept in bounds, and say so ([`Command::SetTariff`]).
    /// What it was set to.
    pub fn set_tariff(&mut self, tariff: economy::Tariff, value: i64) -> i64 {
        let set = self.facilities.economy.set_tariff(tariff, value);
        self.log.important(self.facilities.economy.tariffs().said(tariff));
        set
    }

    /// The dead counted so far. See [`statistics`].
    pub fn tally(&self) -> Tally {
        self.facilities.tally
    }

    /// The world in numbers, worked out now: one walk of the arena, for a
    /// screen to show, never for a tick to read. See [`statistics`].
    pub fn statistics(&self) -> Statistics {
        Statistics::of(&self.entities, &self.facilities, self.clock())
    }

    /// Every door's leaf and lock. See [`door`].
    pub fn doors(&self) -> &Doors {
        &self.facilities.doors
    }

    /// Which cells are whose property. See [`property`].
    pub fn properties(&self) -> &Properties {
        &self.facilities.properties
    }

    /// Which beds belong to whom. See [`homes`].
    pub fn homes(&self) -> &Homes {
        &self.facilities.homes
    }

    /// Which workplaces — desks and scrap bins — belong to whom. See
    /// [`job`].
    pub fn workplaces(&self) -> &Workplaces {
        &self.facilities.workplaces
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

    /// Watched seconds since the world was created — how long somebody has
    /// been looking at it, not how much world has gone by. `f64` because at
    /// 60Hz an `f32` stops being able to represent whole ticks after a few
    /// hours.
    ///
    /// [`GameState::clock`] is the same number in the world's own units, and
    /// is what anything about the world's time of day should ask.
    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }

    /// What time it is in the world: a second of watching is two minutes here
    /// ([`clock::TIME_SCALE`]).
    ///
    /// Derived from [`GameState::elapsed`] rather than counted beside it, so
    /// there is one clock and no way for two to disagree.
    pub fn clock(&self) -> Clock {
        Clock::after_watching(self.elapsed)
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
        let (entity, cash) = {
            // Split off a per-entity stream so the shape of an entity does not
            // depend on how many were made before it.
            let mut rng = SmallRng::seed_from_u64(self.rng.random());
            let entity = kinds::build(uid, kind, at, &mut rng);
            // The stream's last draw, so every roll before it is the one it
            // was before there was money.
            let cash = (kind == EntityType::Human).then(|| self.facilities.economy.tariffs().roll_cash(&mut rng));
            (entity, cash)
        };
        let slot = self.entities.insert(entity);

        // The buffers are indexed by slot, so they have to cover the new one.
        if self.intents.len() <= slot as usize {
            self.intents.resize(slot as usize + 1, Intent::Idle);
            self.moves.resize(slot as usize + 1, MoveOutcome::Idle);
            self.effects.resize(slot as usize + 1, Effect::None);
            self.chatters.resize(slot as usize + 1, None);
            self.smalltalkers.resize(slot as usize + 1, None);
            self.bonders.resize(slot as usize + 1, None);
        }
        self.intents[slot as usize] = Intent::Idle;
        self.moves[slot as usize] = MoveOutcome::Idle;
        self.effects[slot as usize] = Effect::None;
        self.chatters[slot as usize] = None;
        self.smalltalkers[slot as usize] = None;
        self.bonders[slot as usize] = None;

        // Takes the cell if it is free, and leaves whoever is already there in
        // possession if it is not — an entity is placed where it was asked
        // for, so spawning is the one thing that can put two of them in one
        // cell. See the [`occupancy`] module docs; moving cannot.
        let _ = self.occupancy.claim(at, uid);
        // The census has everybody, a second in one cell included.
        self.census.file(slot, at);

        self.log.unit(uid, format!("spawned {uid} at {},{}", at.x, at.y));
        // A person is born into a family: everybody who arrives in the same
        // house ([`relationships`]).
        if kind == EntityType::Human {
            let family = self.facilities.properties.of(at);
            if let Some(entity) = self.entities.get_mut(uid) {
                entity.body_mut().set_family(family);
            }
            self.facilities.society.join(uid, family);
        }
        if let Some(cash) = cash {
            self.facilities.economy.roll(self.clock().day());
            self.facilities.economy.open(uid, cash);
            self.log.unit(uid, format!("{uid} arrived with {cash}"));
        }
        self.give_a_bed(uid, at);
        // After the bed: the key that comes with it says which desks are
        // somewhere it may go.
        self.give_a_job(uid, at);
        uid
    }

    /// Spawn whoever the map's spawners say lives here: one entity per
    /// spawner, of the kind it names, in its cell, in the order the map lists
    /// them. Returns how many arrived.
    ///
    /// Not part of [`GameState::new`], which still brings nobody: a world is
    /// what its caller asked for, and the game screen is the caller that asks
    /// for this. A kind this build does not know is logged and skipped, as an
    /// unknown tile is impassable rather than a panic.
    pub fn spawn_from_spawners(&mut self) -> usize {
        let spawners: Vec<(String, Point)> = self
            .map
            .objects(ObjectLayer::Spawners)
            .iter()
            .map(|spawner| (spawner.kind.as_str().to_string(), spawner.cell()))
            .collect();
        let mut arrived = 0;
        for (name, at) in spawners {
            match EntityType::from_name(&name) {
                Some(kind) => {
                    self.spawn(kind, at);
                    arrived += 1;
                }
                None => self.log.important(format!("no such kind {name:?} to spawn at {},{}", at.x, at.y)),
            }
        }
        if arrived > 0 {
            self.log.push(format!("{arrived} moved in"));
        }
        // A town keeps pets, once everybody is in: after the last arrival, so
        // nobody's roll depends on whether somebody before it has a dog.
        if self.facilities.properties.len() >= feature::TOWN_HOMES {
            self.adopt_pets();
        }
        arrived
    }

    /// Give a pet dog to one resident in [`pets::ONE_IN`] who has a home, by
    /// the random half of their id: the same ones for the same seed. In
    /// the order they arrived.
    fn adopt_pets(&mut self) {
        for owner in self.entities.in_spawn_order() {
            let homed = self.entities.get(owner).is_some_and(|entity| {
                entity.kind() == Some(EntityType::Human) && entity.body().home().is_some()
            });
            if homed && owner.body() % pets::ONE_IN == 0 {
                self.adopt_dog(owner);
            }
        }
    }

    /// Give `owner` a pet dog: a new dog in a free cell beside it, bonded to
    /// it, with its owner's key (so it goes where its owner may), and bonded
    /// back. `None`, and nothing done, for somebody who is not a living person
    /// or already keeps a dog, or with no free cell within three of it.
    pub fn adopt_dog(&mut self, owner: Uid) -> Option<Uid> {
        let entity = self.entities.get(owner)?;
        if entity.kind() != Some(EntityType::Human) || entity.is_dead() || entity.companion().is_some() {
            return None;
        }
        let (at, key) = (entity.center_position(), entity.body().home());
        let beside = (1..=3).find_map(|reach: i32| {
            (-reach..=reach)
                .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
                .filter(|&(dx, dy)| dx.abs().max(dy.abs()) == reach)
                .map(|(dx, dy)| at.offset(dx, dy))
                .find(|&cell| self.map.is_passable(cell) && !self.occupancy.is_occupied(cell))
        })?;
        let dog = self.spawn(EntityType::Dog, beside);
        if let Some(entity) = self.entities.get_mut(dog) {
            entity.set_companion(owner);
            entity.body_mut().set_home(key);
        }
        if let Some(entity) = self.entities.get_mut(owner) {
            entity.set_companion(dog);
        }
        self.log.related(owner, dog, format!("{owner} adopted the dog {dog}"));
        Some(dog)
    }

    /// Hand somebody who has just arrived the nearest bed nobody owns, if it
    /// is the kind that sleeps and there is one. **A bed is only given away
    /// while it is free**: an arrival that finds every bed owned has none, and
    /// will sleep in one only when it is critically tired.
    ///
    /// **A bed in a property comes with its key**: whoever is given one is
    /// an owner of the house it stands in, may open its door and use what is
    /// inside. Somebody with no bed, or one that is nobody's property, owns
    /// nowhere.
    ///
    /// One scan of the beds per arrival — a once-per-spawn cost, and the spawn
    /// pass is the one place that is allowed to have those.
    fn give_a_bed(&mut self, uid: Uid, near: Point) {
        let homes = &self.facilities.homes;
        let Some(bed) = self
            .facilities.features
            .nearest_where(FeatureKind::Bed, near, |cell| !homes.is_owned(cell))
        else {
            return;
        };
        let Some(entity) = self.entities.get_mut(uid) else {
            return;
        };
        let property = self.facilities.properties.of(bed);
        if entity.set_home(bed, property) {
            self.facilities.homes.claim(bed, uid);
            self.log.unit(uid, match property {
                Some(property) => format!("{uid} was given the bed at {}, {} and the key to {property}", bed.x, bed.y),
                None => format!("{uid} was given the bed at {}, {}", bed.x, bed.y),
            });
        }
    }

    /// Hand somebody a job at the nearest free workplace it may use — one
    /// nobody else works at, and not one in somebody else's house — if it is
    /// the kind that works and there is one. **Jobs are offered in
    /// [`JobKind::ALL`]'s order**: a desk if any is free, then a scrap bin,
    /// then a guard post; the first kind with a workplace free wins. **A
    /// workplace is only given while it is free** — a cell and a shift: a
    /// guard post is three, one a watch, and the first watch free at the
    /// nearest post with one is the arrival's. One that finds every one
    /// taken is unemployed.
    ///
    /// One scan of the workplaces per arrival, like
    /// [`GameState::give_a_bed`]. `false` when no job was given.
    fn give_a_job(&mut self, uid: Uid, near: Point) -> bool {
        let Some(entity) = self.entities.get_mut(uid) else {
            return false;
        };
        let key = entity.body().home();
        let (workplaces, properties) = (&self.facilities.workplaces, &self.facilities.properties);
        let Some(job) = JobKind::ALL.into_iter().find_map(|kind| {
            let free = |cell: Point| {
                workplaces.free_shift(cell, kind.shifts()).is_some() && properties.may_use(cell, key)
            };
            let at = self.facilities.features.nearest_where(kind.workplace(), near, free)?;
            Some(Job::new(kind, at, workplaces.free_shift(at, kind.shifts())?))
        }) else {
            return false;
        };
        if !entity.hire(job) {
            return false;
        }
        let at = job.workplace;
        self.facilities.workplaces.claim(at, job.shift, uid);
        // Everybody doing the same is a colleague now.
        self.facilities.society.set_job(uid, Some(job.kind));
        self.log.unit(uid, format!(
            "{uid} was hired as {} at the {} at {}, {}, {}",
            job.kind.with_article(),
            job.kind.workplace_name(),
            at.x,
            at.y,
            job.shift.hours()
        ));
        true
    }

    /// Give `uid` a job at the free workplace nearest where it stands, in
    /// the order arrivals are offered one — see [`Command::Hire`]. `false`,
    /// and a line in the log saying why, for nobody, somebody already
    /// employed, a kind that cannot work, or no workplace free.
    pub fn hire(&mut self, uid: Uid) -> bool {
        let Some(entity) = self.entities.get(uid) else {
            self.log.unit(uid, format!("hire: no such entity {uid}"));
            return false;
        };
        if entity.is_dead() {
            self.log.unit(uid, format!("hire: {uid} is dead"));
            return false;
        }
        if let Some(job) = entity.job() {
            self.log.unit(uid, format!("hire: {uid} is already {}", job.kind.with_article()));
            return false;
        }
        let here = entity.center_position();
        if !self.give_a_job(uid, here) {
            self.log.unit(uid, format!("hire: no workplace free for {uid}"));
            return false;
        }
        true
    }

    /// Take `uid`'s job away and give its workplace back — see
    /// [`Command::Dismiss`]. `false`, and a line in the log, for nobody or
    /// somebody with no job.
    pub fn dismiss(&mut self, uid: Uid) -> bool {
        let Some(entity) = self.entities.get_mut(uid) else {
            self.log.unit(uid, format!("dismiss: no such entity {uid}"));
            return false;
        };
        let Some(job) = entity.dismiss() else {
            self.log.unit(uid, format!("dismiss: {uid} has no job"));
            return false;
        };
        self.facilities.workplaces.release(uid);
        self.facilities.society.set_job(uid, None);
        self.log.unit(uid, format!(
            "{uid} was let go from the {} at {}, {}",
            job.kind.workplace_name(),
            job.workplace.x,
            job.workplace.y
        ));
        true
    }

    /// Remove an entity. Returns whether there was one.
    pub fn despawn(&mut self, uid: Uid) -> bool {
        self.facilities.hospital.release(uid);
        if let Some(slot) = self.entities.slot_of(uid) {
            self.census.strike(slot);
            // The think step writes only live slots, and the chats are
            // gathered from every one: a hole must say nobody is chatting.
            if let Some(chatter) = self.chatters.get_mut(slot as usize) {
                *chatter = None;
            }
            if let Some(smalltalker) = self.smalltalkers.get_mut(slot as usize) {
                *smalltalker = None;
            }
            if let Some(bonder) = self.bonders.get_mut(slot as usize) {
                *bonder = None;
            }
        }
        match self.entities.remove(uid) {
            Some(gone) => {
                // Only if it was the registered occupant: an entity that
                // spawned on top of somebody else must not free their cell on
                // its way out.
                self.occupancy.release(gone.center_position(), uid);
                // Its bed goes back to be given to the next arrival, and its
                // desk.
                self.facilities.homes.release(uid);
                self.facilities.workplaces.release(uid);
                self.facilities.society.leave(uid);
                // What it had, or owed, goes to the budget.
                let _ = self.facilities.economy.close(uid);
                self.log.unit(uid, format!("despawned {uid}"));
                true
            }
            None => false,
        }
    }

    /// Hold an entity still, or let it go again. Returns whether there was
    /// one to freeze.
    ///
    /// A frozen entity keeps its cell — it is standing in it, not removed from
    /// the world — and keeps whatever goal it had, so letting it go again
    /// carries on rather than starting over.
    pub fn set_frozen(&mut self, uid: Uid, frozen: bool) -> bool {
        match self.entities.get_mut(uid) {
            Some(entity) => {
                entity.set_frozen(frozen);
                let verb = if frozen { "froze" } else { "unfroze" };
                self.log.unit(uid, format!("{verb} {uid}"));
                true
            }
            None => false,
        }
    }

    /// Switch one of an entity's biological processes on or off. `false`, and
    /// a line in the log saying why, for an entity that is not there or has no
    /// biology to switch.
    pub fn set_process(&mut self, uid: Uid, process: ProcessId, running: bool) -> bool {
        let name = process.name();
        let Some(entity) = self.entities.get_mut(uid) else {
            self.log.unit(uid, format!("{name}: no such entity {uid}"));
            return false;
        };
        let Some(biology) = entity.biology_mut() else {
            self.log.unit(uid, format!("{name}: {uid} has no biology"));
            return false;
        };
        biology.set_running(process, running);
        let state = if running { "on" } else { "off" };
        self.log.unit(uid, format!("switched {name} {state} for {uid}"));
        true
    }

    /// Stow an item in an entity's inventory. `false`, and a line in the log
    /// saying why, for an entity that is not there, has nothing to carry
    /// things in, or has no room left for this one.
    ///
    /// A refusal is the inventory's own ([`Inventory::stow`]): giving
    /// something to a unit that cannot take it has to fail visibly, or a limit
    /// is only a limit until somebody hands you one more.
    pub fn give_item(&mut self, uid: Uid, item: ItemKind) -> bool {
        let name = item.name();
        let Some(entity) = self.entities.get_mut(uid) else {
            self.log.unit(uid, format!("give {name}: no such entity {uid}"));
            return false;
        };
        let Some(inventory) = entity.inventory_mut() else {
            self.log.unit(uid, format!("give {name}: {uid} carries nothing"));
            return false;
        };
        if !inventory.stow(item) {
            let load = inventory.describe_load();
            self.log.unit(uid, format!("{uid} has no room for {name}: {load}"));
            return false;
        }
        self.log.unit(uid, format!("gave {name} to {uid}"));
        true
    }

    /// Put an item in an entity's hand, or empty it. `false`, and a line in
    /// the log saying why, for an entity that is not there or has no hands.
    pub fn hold_item(&mut self, uid: Uid, item: Option<ItemKind>) -> bool {
        let Some(entity) = self.entities.get_mut(uid) else {
            self.log.unit(uid, format!("hold: no such entity {uid}"));
            return false;
        };
        let Some(inventory) = entity.inventory_mut() else {
            self.log.unit(uid, format!("hold: {uid} carries nothing"));
            return false;
        };
        let _ = inventory.set_hand(item);
        self.log.unit(uid, match item {
            Some(item) => format!("{uid} is holding {}", item.name()),
            None => format!("{uid} is holding nothing"),
        });
        true
    }

    /// Tell `uid` to salvage the heap of scrap at `at` — see
    /// [`Command::Salvage`]. `false`, and a line in the log saying why, when
    /// there is no heap there with anything left in it, no such entity, or
    /// one that cannot salvage (a dog). The order is the unit's to carry out
    /// from here: it walks there, works the heap, and lets the order go when
    /// it is empty, when it can carry no more, or when there is no getting
    /// there.
    pub fn order_salvage(&mut self, uid: Uid, at: Point) -> bool {
        let Some(heap) = self.map.scrap(at) else {
            self.log.unit(uid, format!("salvage: no scrap at {}, {}", at.x, at.y));
            return false;
        };
        let Some(entity) = self.entities.get_mut(uid) else {
            self.log.unit(uid, format!("salvage: no such entity {uid}"));
            return false;
        };
        if !entity.order_salvage(at) {
            self.log.unit(uid, format!("salvage: {uid} cannot salvage anything"));
            return false;
        }
        self.log.unit(uid, format!(
            "{uid} was told to salvage the scrap at {}, {}: {} left",
            at.x, at.y, heap.pieces
        ));
        true
    }

    /// Explicitly change one talent, including on a frozen person. Returns
    /// false for a missing entity or a kind without talents.
    pub fn set_talent(&mut self, uid: Uid, talent: Talent, value: i32) -> bool {
        let Some(entity) = self.entities.get_mut(uid) else {
            self.log.unit(uid, format!("set talent: no such entity {uid}"));
            return false;
        };
        let Some(talents) = entity.talents_mut() else {
            self.log.unit(uid, format!("set talent: {uid} has no talents"));
            return false;
        };
        talents.set(talent, value);
        self.log.unit(uid, format!("{uid} {} talent set to {}", talent.name(), talents.get(talent)));
        true
    }

    /// Change an outfit even while paused or frozen. Kinds without clothing
    /// and ids that have left the world refuse the request.
    pub fn set_clothing(&mut self, uid: Uid, clothing: clothing::Clothing) -> bool {
        let Some(worn) = self.entities.get_mut(uid).and_then(|entity| entity.clothing_mut()) else {
            self.log.unit(uid, format!("set clothing: {uid} has no clothing"));
            return false;
        };
        *worn = clothing;
        self.log.unit(uid, format!("{uid} clothing set to {}", clothing.name()));
        true
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
/// What `map` supplies with the boxes in `boxes_off` switched off by hand,
/// the controllers' boxes in `controlled` on or off as `street_lights_on`
/// says, and the lamps in `dark` off at their light switches.
fn flood_supply(
    map: &Map,
    boxes_off: &[Point],
    controlled: &[Point],
    street_lights_on: bool,
    dark: impl IntoIterator<Item = Point>,
) -> Supply {
    let mut off = boxes_off.to_vec();
    if !street_lights_on {
        off.extend_from_slice(controlled);
    }
    Supply::with_boxes_off(map, &off).with_lights_off(dark)
}

/// The one function. See the module docs for why it takes `&mut` rather than
/// returning a new state. The two passes are public in their own right, for a
/// caller that wants to spawn a world once and then run it — which is what the
/// QA harness's `tick` step does.
pub fn process_game_state(state: &mut GameState, dt: f32, input: &Input) {
    spawn_pass(state, input);
    process_pass(state, dt);
}

/// **Who leaves the world and who comes into it, at the end of a processing
/// pass** — after its four steps, which add and remove nobody, once the
/// frozen table is let go, so nothing is left waiting between ticks and
/// nothing new is saved.
///
/// 1. Everybody leaving goes ([`Facilities::leaving`]): the bodies
///    undertakers took up this tick ([`Effect::TakeBody`], in a hand now as
///    an item) and the deathdogs gone back into the wasteland
///    ([`Effect::Depart`]).
/// 2. On the tick it gets dark (`before` light, `now` dark), the night's
///    deathdogs come out of the wasteland ([`deathdogs::arrive`]).
///
/// Inside [`process_pass`] and not round it: the game and the QA harness
/// drive the two passes apart, and what happens on a tick has to happen on
/// that tick, however the ticks fall into frames.
fn comings_and_goings(state: &mut GameState, before: Clock, now: Clock) {
    let mut leaving = std::mem::take(&mut state.facilities.leaving);
    for uid in leaving.drain(..) {
        state.despawn(uid);
    }
    state.facilities.leaving = leaving;
    if deathdogs::is_dusk(before, now) {
        deathdogs::arrive(state);
    }
}

/// **The spawn pass: the only thing that changes which entities exist.**
///
/// Spawns and despawns, from [`Input::commands`]. It runs before the
/// processing pass, so something spawned this tick thinks this tick — watching
/// a new entity stand still for a frame reads as a bug in whatever spawned it.
///
/// [`Command::Freeze`], [`Command::SetProcess`], [`Command::GiveItem`],
/// [`Command::Hold`], [`Command::SetClothing`] and [`Command::SetTalent`] are drained here too. They
/// change nothing structural, but arrive on the same queue and want the same
/// timing: a freeze asked for this tick is in
/// force before anything thinks, rather than one tick after the button was
/// pressed.
///
/// This is also the only pass that may resize the entity table, and therefore
/// the only one that may allocate. [`GameState::spawn`] keeps the intent
/// buffer sized to the arena as it goes, which is what lets
/// [`process_pass`] index it without a bounds concern and without touching it.
pub fn spawn_pass(state: &mut GameState, input: &Input) {
    state.facilities.economy.roll(state.clock().day());
    if state.facilities.electrical.supply_dirty {
        state.refresh_supply();
    }
    for command in &input.commands {
        match command {
            Command::SetDirt { cell, value } => {
                if !state.facilities.dirt.set(*cell, *value) { state.log.important("invalid ground dirt vector"); }
            }
            Command::SetUnitDirt { uid, value } => {
                if let Some(entity) = state.entities.get_mut(*uid) {
                    if !entity.body_mut().set_dirt(*value) { state.log.important("invalid unit dirt vector"); }
                }
            }
            Command::SetClothing { uid, clothing } => {
                state.set_clothing(*uid, *clothing);
            }
            Command::Spawn { kind, at } => {
                if kind.keeps_off_the_site() && state.map.site().contains(*at) {
                    // It could never step off it again, nor be routed
                    // anywhere: a route for one goes round the site.
                    state.log.important(format!("a {kind} keeps off the site: none spawned at {}, {}", at.x, at.y));
                } else {
                    state.spawn(*kind, *at);
                }
            }
            Command::Despawn(uid) => {
                if !state.despawn(*uid) {
                    state.log.unit(*uid, format!("despawn: no such entity {uid}"));
                }
            }
            Command::SwitchBox { at, on } => {
                if !state.switch_box(*at, *on) {
                    state.log.important(format!("switch: no distribution box at {}, {}", at.x, at.y));
                }
            }
            Command::SwitchLight { at, on } => {
                if !state.switch_light(*at, *on) {
                    state.log.important(format!("switch: no light switch at {}, {}", at.x, at.y));
                }
            }
            Command::Freeze { uid, frozen } => {
                if !state.set_frozen(*uid, *frozen) {
                    state.log.unit(*uid, format!("freeze: no such entity {uid}"));
                }
            }
            Command::SetProcess { uid, process, running } => {
                state.set_process(*uid, *process, *running);
            }
            Command::SetTalent { uid, talent, value } => {
                state.set_talent(*uid, *talent, *value);
            }
            Command::GiveItem { uid, item } => {
                state.give_item(*uid, *item);
            }
            Command::Hold { uid, item } => {
                state.hold_item(*uid, *item);
            }
            Command::Salvage { uid, at } => {
                state.order_salvage(*uid, *at);
            }
            Command::Hire(uid) => {
                state.hire(*uid);
            }
            Command::Dismiss(uid) => {
                state.dismiss(*uid);
            }
            Command::SetTariff { tariff, value } => {
                state.set_tariff(*tariff, *value);
            }
            Command::SetRelationshipRule { rule, value } => {
                state.set_relationship_rule(*rule, *value);
            }
        }
    }
    state.follow_the_crowd();
}

/// **The processing pass: advances every entity, and adds or removes none.**
///
/// Four steps — [`think_step`], [`move_step`], [`react_step`], [`world_step`]
/// — over a table whose membership is frozen: it is handed a
/// [`FrozenEntities`], which has no `insert` and no `remove`, so this is a
/// guarantee the compiler holds rather than a rule to remember. The four add
/// and remove nobody; once they are done and the table is let go, the
/// comings and goings of the tick follow ([`comings_and_goings`]): whoever
/// is leaving goes, and at dusk the night's deathdogs come.
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
    // What time it was as the last tick ended: what tells the tick it gets
    // dark from the ticks after it.
    let before = state.clock();
    state.tick += 1;
    state.elapsed += dt as f64;
    // Read once, and shared by both rounds: the clock does not move inside a
    // tick, and every entity should see the same time.
    let clock = state.clock();
    // Whole world hours, derived from the saved clock rather than a second
    // timer. Catch up every crossed hour even when a caller uses a large dt.
    let hours_before = (before.elapsed() / clock::HOUR as f64).floor() as u64;
    let hours_now = (clock.elapsed() / clock::HOUR as f64).floor() as u64;
    if hours_now > hours_before {
        state.facilities.dirt.evaporate(hours_now - hours_before);
    }
    // Dusk or dawn on this very tick: the street lights change before
    // anybody thinks, so the whole crowd sees the same supply.
    state.follow_daylight(clock);

    // Disjoint field borrows, so the borrow checker is what proves the steps
    // below do not overlap: think reads the world and writes `intents`, move
    // reads `intents` and writes the entities and `occupancy`, react reads
    // `moves` and writes the entities.
    let GameState {
        map,
        entities,
        occupancy,
        census,
        log,
        facilities,
        tick,
        intents,
        moves,
        effects,
        chatters,
        smalltalkers,
        bonders,
        route_work,
        ..
    } = &mut *state;

    let mut table = entities.freeze();

    // Sized by the spawn pass, which is the only thing that can change the
    // arena. If this ever trips, something grew the table outside that pass.
    debug_assert!(
        intents.len() >= table.capacity()
            && moves.len() >= table.capacity()
            && effects.len() >= table.capacity()
            && chatters.len() >= table.capacity()
            && smalltalkers.len() >= table.capacity()
            && bonders.len() >= table.capacity(),
        "the slot buffers ({}, {}, {}, {}, {}, {}) are shorter than the entity table ({})",
        intents.len(),
        moves.len(),
        effects.len(),
        chatters.len(),
        smalltalkers.len(),
        bonders.len(),
        table.capacity()
    );

    // The crowd as it ended the previous tick.
    think_step(
        &table,
        &Think::new(map, occupancy, log, facilities, dt, *tick, clock),
        intents,
        chatters,
        smalltalkers,
        bonders,
    );
    // Who is chatting with whom, as the think step found everybody: what
    // react asks to know who is asking it for a chat and where a partner is.
    // Small talk the same way.
    facilities.chats.rebuild(&chatters[..table.capacity()]);
    facilities.smalltalks.rebuild(&smalltalkers[..table.capacity()]);
    // Who is bonded to whom, the same way: where an owner is for its dog to
    // stay by, and how hungry the dog is.
    facilities.bonds.rebuild(&bonders[..table.capacity()]);

    move_step(
        &mut table,
        map,
        occupancy,
        census,
        &facilities.features,
        &facilities.doors,
        &mut facilities.panics,
        &mut facilities.dirt,
        intents,
        moves,
    );

    // ...and as it ends this one, which is the layer a detour is worth
    // planning against. Same struct, deliberately different moment.
    *route_work = react_step(
        &mut table,
        &Think::new(map, occupancy, log, facilities, dt, *tick, clock),
        moves,
        effects,
    );

    // **Step 4: the world settles what react asked of it, then the fridges
    // and the doors move on.** Sequential, in slot order, for the same reason
    // the move step is: two entities opening and closing one fridge on the
    // same tick has to settle the same way on every run. A fridge's
    // temperature is on the world's clock, like everything biological, and a
    // door's leaf on the watched one, like a walk — see `clock`.
    world_step(&mut table, map, log, facilities, occupancy, effects, dt, clock);

    // The table is let go: whoever is leaving goes, and at dusk the night's
    // deathdogs come.
    comings_and_goings(state, before, clock);
}

/// Live entities below which the rounds that could run in parallel do not
/// bother.
///
/// Handing eight entities to a thread pool costs more than thinking for them,
/// and every test in this crate and most of the QA suite runs worlds that
/// size. The answer is identical either side of the line — the parallel rounds
/// write one slot each and read nothing that changes, and react shares its
/// route budget out by blocks fixed by slot number, the same either way — so
/// this is a choice about overhead and never about behaviour.
const PARALLEL_AT: usize = 64;

/// **Step 1: everybody decides.** Read-only, and run across threads.
///
/// Nothing is written but the intent buffer, and each entity writes only its
/// own slot, so the arena and the buffer are zipped index for index and handed
/// to rayon: two threads never touch the same `Intent` and nothing they read
/// changes while they read it. Keep it that way — an entity that wants to see
/// what another entity decided this tick is asking for the one thing this
/// shape cannot give.
///
/// Each entity also says what chat and what small talk it is in, if any,
/// into `chatters` and `smalltalkers` — asked here because this is the one
/// walk of the whole crowd that already has every unit in cache
/// ([`relationships::Chats`], [`relationships::Smalltalks`]), and who it is
/// bonded to if it is somebody's pet or has one ([`pets::Bonds`]).
fn think_step(
    table: &FrozenEntities<'_>,
    ctx: &Think<'_>,
    intents: &mut [Intent],
    chatters: &mut [Option<relationships::Chatter>],
    smalltalkers: &mut [Option<relationships::Smalltalker>],
    bonders: &mut [Option<pets::Bond>],
) {
    let intents = &mut intents[..table.capacity()];
    let chatters = &mut chatters[..table.capacity()];
    let smalltalkers = &mut smalltalkers[..table.capacity()];
    let bonders = &mut bonders[..table.capacity()];
    if table.len() < PARALLEL_AT {
        for (slot, entity) in table.iter_slots() {
            let slot = slot as usize;
            (intents[slot], chatters[slot], smalltalkers[slot], bonders[slot]) = think_for(entity, ctx);
        }
        return;
    }
    intents
        .par_iter_mut()
        .zip(chatters.par_iter_mut())
        .zip(smalltalkers.par_iter_mut())
        .zip(bonders.par_iter_mut())
        .zip(table.par_iter_slots())
        .for_each(|((((intent, chatter), smalltalker), bonder), entity)| match entity {
            Some(entity) => (*intent, *chatter, *smalltalker, *bonder) = think_for(entity, ctx),
            // A hole talks with nobody, whoever left it.
            None => (*chatter, *smalltalker, *bonder) = (None, None, None),
        });
}

/// What one entity decides this tick, whichever round asked it.
///
/// A frozen entity is not asked. Whatever it decided would be thrown away by a
/// move step that is not going to move it, and the intent it did not produce
/// is the [`Intent::Idle`] the move step wants anyway — so the freeze costs a
/// bool per entity here rather than a decision per entity everywhere else.
///
/// And the chat and small talk it is in: none for the frozen and the dead,
/// who are in no state to talk, so whoever they were talking with gives up on
/// them.
fn think_for(
    entity: &dyn GameEntity,
    ctx: &Think<'_>,
) -> (Intent, Option<relationships::Chatter>, Option<relationships::Smalltalker>, Option<pets::Bond>) {
    if entity.is_dead() {
        (Intent::Idle, None, None, None)
    } else if entity.is_frozen() {
        // Held still, and still where it stands: a dog keeps its frozen
        // owner's place, and the owner its dog's.
        (Intent::Idle, None, None, entity.bond(ctx))
    } else {
        (entity.think(ctx), entity.chatter(), entity.smalltalker(), entity.bond(ctx))
    }
}

/// **Step 2: everybody moves, one at a time, and the world may say no.**
///
/// Sequential and in ascending slot order, because it writes the occupancy
/// layer: two entities heading for the same empty cell on the same tick is the
/// race this exists to settle, and it settles it the same way on every run.
///
/// An [`Intent::Move`] is granted only if the destination cell is passable
/// terrain — or a feature's cell whose [`feature::Access`] is `Entered`, the
/// one deliberate exception — *and* nobody else is standing in
/// it. Otherwise the move is **cancelled outright** — no partial step, no
/// sliding along the wall — and the entity is told what stopped it.
/// Cancelling rather than trimming is what makes the outcome meaningful: "you
/// did not get there, and this is who was in the way" is something an entity
/// can act on, where "you got 40% of the way" is not.
///
/// A step that stays inside the entity's own cell is the common case by a wide
/// margin — at a walking pace of four cells a second, a boundary is crossed
/// about once every fifteen ticks — and it costs neither of the two checks: no
/// cell changes hands, so there is nothing to claim and nobody new to bump
/// into. A step into a new cell is also where the [`Census`] hears of it,
/// which matters once every thirty-odd cells, when it crosses into another
/// chunk.
// The world's parts, borrowed apart so the borrow checker can see which of
// them the step writes.
#[allow(clippy::too_many_arguments)]
fn move_step(
    table: &mut FrozenEntities<'_>,
    map: &Map,
    occupancy: &mut Occupancy,
    census: &mut Census,
    features: &Features,
    doors: &Doors,
    panics: &mut panics::Panics,
    dirt: &mut dirt::DirtLayer,
    intents: &[Intent],
    moves: &mut [MoveOutcome],
) {
    panics.clear();
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
        // Seen running for its life, whether or not the world lets it
        // through: one bool off a body already in cache ([`panics`]).
        if entity.body().is_panicking() {
            panics.push(uid);
        }

        moves[slot] = if into == from {
            // Still in its own cell: it already holds this one.
            entity.apply(&intent);
            MoveOutcome::Moved
        } else if uid.kind().is_some_and(EntityType::keeps_off_the_site) && map.site().contains(into) {
            // A deathdog never sets foot on the site. Its routes go round it
            // (`Think::step_cost_with`), so this is the guarantee rather than
            // the mechanism.
            MoveOutcome::Blocked { by: None }
        } else if !map.is_passable(into) && !features.is_enterable(into) {
            // The terrain first — a wall is cheaper to hit than a crowd, and
            // off the map is impassable, so this is the bounds check too.
            // An `Access::Entered` feature's cell is the one exception: it
            // reads as impassable here on purpose (a route never targets it,
            // so nobody walks through it as a shortcut) but is allowed
            // through to the claim below, which is what actually decides
            // whether this particular step is granted.
            MoveOutcome::Blocked { by: None }
        } else if doors.is_shut(into) {
            // A door not open all the way is a wall. A walker never asks to
            // step into one — it stands and waits (`Walker::think`) — so
            // this is the guarantee rather than the mechanism.
            MoveOutcome::Blocked { by: None }
        } else if let Err(other) = occupancy.claim(into, uid) {
            // A refused claim writes nothing, so there is nothing to undo.
            MoveOutcome::Blocked { by: Some(other) }
        } else {
            // Claim first and release second: the other order would leave the
            // old cell open for a moment, which matters the day this runs
            // anywhere but here.
            occupancy.release(from, uid);
            dirt.enter(into, entity.body_mut().dirt_mut());
            entity.apply(&intent);
            census.moved(slot as Slot, into);
            MoveOutcome::Moved
        };
    }
    panics.finish();
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
/// separate channel. The default [`GameEntity::react`] does nothing and asks
/// for nothing, so a kind with no plan pays a call and no more.
///
/// Each entity writes only to itself — including the [`Effect`] it hands
/// back, one slot each, the same shape `intents` already is — so this runs
/// across threads the same way think does: a `&mut` walk of the table zipped
/// with `effects` rather than a second intent buffer nobody would read, and
/// rayon over disjoint slots above [`PARALLEL_AT`].
///
/// **This is where pathfinding happens**, and it is the round that most wants
/// the threads: a far route is a search over the map, one walker's search
/// shares nothing with another's, and a crowd that has all arrived at once
/// wants all of them planned at once. See [`walker::Walker`]; and since the
/// brain runs here too, [`brain`] for what else a reaction decides.
///
/// # The route budget
///
/// What the whole crowd may spend on far routes in one tick is bounded:
/// [`ROUTE_BUDGET`] cells expanded, shared out. Without it, a crowd that all
/// wanted to go somewhere far at once — dusk, and everybody off to bed —
/// searched it all in one tick, and the frame waited for every search.
///
/// Shared out **by block**, not by who asks first. Who asks first is a race
/// once react runs on threads, and the simulation does not run races. So the
/// slots are cut into blocks of [`ROUTE_BLOCK`], each block gets an equal
/// share ([`MIN_BLOCK_BUDGET`] at least), and a block reacts in order on one
/// thread, its units spending from the block's share as they go. The blocks
/// run in parallel; which slots make a block is fixed by slot number alone,
/// so the sequential path below a crowd of [`PARALLEL_AT`] cuts them the
/// same and gives the same answer. Within a block the turn rotates with the
/// tick, so nobody is always last to the budget.
///
/// **A budget delays a route; it never changes one.** A search that runs out
/// carries on next tick from where it stopped, and finds the route an
/// unlimited search would have ([`path::Allowance`]). Nothing here knows or
/// asks what is on the screen: a unit a long way off gets its turn exactly as
/// one in view does.
///
/// An unfinished search keeps scratch the size of its window between ticks,
/// so a block keeps at most [`SEARCHES_KEPT`]. A unit that wants to start
/// another while that one is held gets [`ROUTE_TRIAL`] cells — plenty for a
/// walk across a room — and if that is not enough its work is let go and it
/// asks again next tick; once the held search is done, the next in turn
/// takes its place.
///
/// Detours round a body ([`walker::Walker::detour`]) are not drawn from it:
/// they plan against where the crowd stands this tick, so they cannot wait
/// for another, and each is bounded by [`walker::DETOUR_LIMIT`] already.
///
/// Returns the cells the far searches expanded.
fn react_step(table: &mut FrozenEntities<'_>, ctx: &Think<'_>, moves: &[MoveOutcome], effects: &mut [Effect]) -> usize {
    let capacity = table.capacity();
    let moves = &moves[..capacity];
    let effects = &mut effects[..capacity];
    let share = (ROUTE_BUDGET / capacity.div_ceil(ROUTE_BLOCK).max(1)).max(MIN_BLOCK_BUDGET);
    let turn = ctx.tick as usize;
    if table.len() < PARALLEL_AT {
        return table
            .blocks_mut(ROUTE_BLOCK)
            .zip(moves.chunks(ROUTE_BLOCK))
            .zip(effects.chunks_mut(ROUTE_BLOCK))
            .map(|((block, moves), effects)| react_block(block, ctx, moves, effects, share, turn))
            .sum();
    }
    table
        .par_blocks_mut(ROUTE_BLOCK)
        .zip(moves.par_chunks(ROUTE_BLOCK))
        .zip(effects.par_chunks_mut(ROUTE_BLOCK))
        .map(|((block, moves), effects)| react_block(block, ctx, moves, effects, share, turn))
        .sum()
}

/// The far-route search the whole world may do in one tick, in cells
/// expanded — see [`react_step`]. Four times what one walker may do
/// ([`walker::FAR_LIMIT`]), so a small world searches as it always did.
pub const ROUTE_BUDGET: usize = 1 << 18;

/// How many slots share one part of [`ROUTE_BUDGET`], reacting in turn on
/// one thread. The size [`PARALLEL_AT`] is, for the same reason: a smaller
/// piece of work is not worth handing to a thread.
pub const ROUTE_BLOCK: usize = 64;

/// The least a block is given, however many there are: enough that a long
/// search in a crowd of thousands still gets somewhere every tick. With very
/// many blocks this, not [`ROUTE_BUDGET`], is what bounds the tick.
pub const MIN_BLOCK_BUDGET: usize = 1 << 12;

/// Unfinished far searches a block keeps between ticks. One: each holds
/// scratch the size of its window, a few megabytes for a search about a
/// large map.
pub const SEARCHES_KEPT: usize = 1;

/// What a unit gets to try a far route with while its block is already
/// holding [`SEARCHES_KEPT`] unfinished ones: enough for any walk across a
/// room or down a street, which is most of them.
pub const ROUTE_TRIAL: usize = 256;

/// One block's reactions, in turn from the slot `turn` points at, sharing
/// `share` cells of route search between them. Returns what they spent.
fn react_block(
    mut block: entities::SlotBlock<'_>,
    ctx: &Think<'_>,
    moves: &[MoveOutcome],
    effects: &mut [Effect],
    share: usize,
    turn: usize,
) -> usize {
    let len = block.len();
    let mut left = share;
    let mut kept = (0..len)
        .filter(|&index| block.get(index).and_then(|entity| entity.walker()).is_some_and(Walker::is_searching))
        .count();
    for offset in 0..len {
        let index = (turn + offset) % len;
        effects[index] = match block.get_mut(index) {
            Some(entity) => react_for(entity, ctx, moves[index], &mut left, &mut kept),
            None => Effect::None,
        };
    }
    share - left
}

/// One entity's reaction, whichever round asked for it — [`think_for`]'s
/// other half — with what is `left` of its block's route budget, and how
/// many unfinished searches the block is holding (`kept`), both brought up to
/// date with what it did.
///
/// A frozen entity is not asked here either, and it matters more here than
/// in think: its brain would find no action running, and an executor asked
/// for work every tick by a unit that can never walk what it plans is a fresh
/// search every tick for nothing. Skipping is also more faithful to what
/// freezing claims — "it keeps its cell and its goal" — than a brain that
/// replans while held still. Its needs are held with it: no time passes for a
/// frozen body, and it asks the world for nothing either.
fn react_for(
    entity: &mut dyn GameEntity,
    ctx: &Think<'_>,
    outcome: MoveOutcome,
    left: &mut usize,
    kept: &mut usize,
) -> Effect {
    // The dead the same, and for good.
    if entity.is_frozen() || entity.is_dead() {
        return Effect::None;
    }
    let Some(walker) = entity.walker_mut() else {
        return entity.react(ctx, outcome);
    };
    let held = walker.is_searching();
    let may_keep = held || *kept < SEARCHES_KEPT;
    let given = if may_keep { *left } else { (*left).min(ROUTE_TRIAL) };
    walker.meter(path::Allowance { cells: given, may_keep });
    let effect = entity.react(ctx, outcome);
    let walker = entity.walker_mut().expect("it walked a moment ago");
    *left -= given - walker.unmeter();
    *kept = *kept + usize::from(walker.is_searching()) - usize::from(held);
    effect
}

/// **Step 4: the world settles what react asked of it, and the fridges move
/// on.**
///
/// Sequential, in slot order — the same reason [`move_step`] is: two units
/// opening and closing the same fridge on the same tick is a race, and this
/// is what settles it the same way on every run. Every effect is applied
/// ([`effect::apply`]) and the slot reset to [`Effect::None`] — react always overwrites
/// its own slot, but a slot whose entity despawned keeps whatever it was
/// last set to, so resetting here rather than relying on the next write is
/// what keeps a stale effect from being replayed.
///
/// Then every fridge's temperature moves by `game_dt`, once, regardless of
/// how many doors just changed — a fridge that was opened and closed in the
/// same tick still ages by the tick's worth of world time.
///
/// Every leaf of every door then moves by the **watched** `dt` — a door is
/// seen to open, like a walk — and none shuts on a body standing in it, which
/// is why the crowd as it ends the tick is handed in.
///
/// What each effect does is [`effect::apply`]'s: a piece off a heap and into
/// a pack, a packful into a store, a body into a hand, all of them writes into
/// the world and the unit together, which is why this step — the one that
/// settles races between units, in slot order — is where they happen.
#[allow(clippy::too_many_arguments)]
fn world_step(
    table: &mut FrozenEntities<'_>,
    map: &mut Map,
    log: &Log,
    facilities: &mut Facilities,
    occupancy: &mut Occupancy,
    effects: &mut [Effect],
    dt: f32,
    clock: Clock,
) {
    // A new day's book, before anything is paid or bought on it.
    facilities.economy.roll(clock.day());
    // Who runs a playzone is said afresh every tick (`Effect::Operate`).
    facilities.playzones.begin_tick();
    // Settle bodily stains before forgetting wounds or granting new bites.
    // This also leaves the unit's own effect free for work or death.
    for (_, entity) in table.iter_slots_mut() {
        soiling::settle(entity, facilities, log);
    }
    // Last tick's bites have been felt, in the reactions just run.
    facilities.wounds.begin_tick();
    // ...and so have last tick's meals served, by the dogs fed.
    facilities.feedings.begin_tick();
    for (slot, effect) in effects.iter_mut().enumerate() {
        let asked = std::mem::take(effect);
        if asked != Effect::None
            && let Some(entity) = table.slot_mut(slot as Slot)
        {
            effect::apply(asked, entity, map, facilities, occupancy, log);
        }
    }
    facilities.fridges.advance(dt * clock::TIME_SCALE);
    facilities.doors.advance(dt, occupancy);
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
    fn a_hold_command_fills_and_empties_a_hand_and_nothing_else() {
        let mut state = world();
        let uid = state.spawn(EntityType::Human, Point::new(3, 3));
        let hand = |state: &GameState| state.entities().get(uid).unwrap().inventory().unwrap().hand();

        let mut input = Input::new();
        input.hold(uid, Some(ItemKind::Water));
        spawn_pass(&mut state, &input);
        assert_eq!(hand(&state), Some(ItemKind::Water));
        // The hand, and not the pockets: stowing is `GiveItem`'s business.
        let entity = state.entities().get(uid).unwrap();
        assert_eq!(entity.inventory().unwrap().count(ItemKind::Water), 0);

        // Whatever was there is replaced rather than refused, and `None` empties it.
        let mut input = Input::new();
        input.hold(uid, Some(ItemKind::Food));
        spawn_pass(&mut state, &input);
        assert_eq!(hand(&state), Some(ItemKind::Food));

        let mut input = Input::new();
        input.hold(uid, None);
        spawn_pass(&mut state, &input);
        assert_eq!(hand(&state), None);
    }

    #[test]
    fn holding_something_is_refused_with_a_reason_when_there_are_no_hands() {
        let mut state = world();
        let dog = state.spawn(EntityType::Dog, Point::new(3, 3));
        let gone = state.spawn(EntityType::Human, Point::new(4, 4));
        state.despawn(gone);

        assert!(!state.hold_item(dog, Some(ItemKind::Food)));
        assert!(!state.hold_item(gone, Some(ItemKind::Food)));
        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("carries nothing")), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("no such entity")), "{lines:?}");
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
    fn a_frozen_entity_stays_exactly_where_it_was() {
        let mut state = world();
        let uid = state.spawn(EntityType::Human, Point::new(4, 4));

        // Long enough to be sure it would have wandered off on its own.
        run(&mut state, 200);
        let wandered_to = state.position_of(uid).expect("still alive");

        let mut input = Input::new();
        input.freeze(uid, true);
        process_game_state(&mut state, 1.0 / 60.0, &input);
        run(&mut state, 500);

        assert_eq!(state.position_of(uid), Some(wandered_to));
        // Frozen, not gone: it is still in the world and still holds its cell.
        assert_eq!(state.len(), 1);
        assert_eq!(
            state.occupancy().occupant(cell_of(wandered_to)),
            Some(uid),
        );
    }

    #[test]
    fn letting_a_frozen_entity_go_starts_it_moving_again() {
        let mut state = world();
        let uid = state.spawn(EntityType::Dog, Point::new(4, 4));

        let mut freeze = Input::new();
        freeze.freeze(uid, true);
        process_game_state(&mut state, 1.0 / 60.0, &freeze);
        run(&mut state, 100);
        let held_at = state.position_of(uid).expect("still alive");

        let mut thaw = Input::new();
        thaw.freeze(uid, false);
        process_game_state(&mut state, 1.0 / 60.0, &thaw);
        run(&mut state, 100);

        assert_ne!(state.position_of(uid), Some(held_at));
    }

    fn hunger_of(state: &GameState, uid: Uid) -> f32 {
        let entity = state.entities().get(uid).expect("still alive");
        entity.biology().expect("a human has a body").stats().hunger()
    }

    #[test]
    fn outfits_can_change_without_changing_the_person_or_advancing_time() {
        use clothing::Clothing;
        let mut state = world();
        let uid = state.spawn(EntityType::Human, Point::new(4, 4));
        let other = state.spawn(EntityType::Human, Point::new(5, 4));
        let person = state.entities().get(uid).unwrap();
        let seed = person.appearance_seed();
        let name = person.display_name();
        let talents = *person.talents().unwrap();
        let other_outfit = state.entities().get(other).unwrap().clothing();
        let mut input = Input::new();
        input.freeze(uid, true).set_clothing(uid, Clothing::GownBlack);
        spawn_pass(&mut state, &input);
        assert_eq!(state.tick(), 0);
        assert_eq!(state.entities().get(uid).unwrap().clothing(), Some(Clothing::GownBlack));
        run(&mut state, 100);
        let person = state.entities().get(uid).unwrap();
        assert_eq!(person.clothing(), Some(Clothing::GownBlack));
        assert_eq!(person.appearance_seed(), seed);
        assert_eq!(person.display_name(), name);
        assert_eq!(*person.talents().unwrap(), talents);
        assert_eq!(state.entities().get(other).unwrap().clothing(), other_outfit);
        input.clear();
        input.set_clothing(uid, Clothing::None);
        spawn_pass(&mut state, &input);
        assert_eq!(state.entities().get(uid).unwrap().clothing(), Some(Clothing::None));
        input.clear();
        input.set_clothing(uid, Clothing::SwimsuitBlue);
        spawn_pass(&mut state, &input);
        assert_eq!(state.entities().get(uid).unwrap().clothing(), Some(Clothing::SwimsuitBlue));
    }

    #[test]
    fn clothing_commands_refuse_dogs_and_missing_units() {
        let mut state = world();
        let dog = state.spawn(EntityType::Dog, Point::new(4, 4));
        assert_eq!(state.entities().get(dog).unwrap().clothing(), None);
        assert!(!state.set_clothing(dog, clothing::Clothing::GownBlack));
        assert!(!state.set_clothing(Uid::new(EntityType::Human, 99), clothing::Clothing::None));
    }

    #[test]
    fn talents_are_reproducible_stable_over_time_and_mutable_by_command() {
        let mut a = world();
        let mut b = world();
        let uid = a.spawn(EntityType::Human, Point::new(4, 4));
        let replay = b.spawn(EntityType::Human, Point::new(4, 4));
        let before = *a.entities().get(uid).unwrap().talents().unwrap();
        assert_eq!(before, *b.entities().get(replay).unwrap().talents().unwrap());
        run(&mut a, 1000);
        assert_eq!(before, *a.entities().get(uid).unwrap().talents().unwrap());

        let mut input = Input::new();
        input.freeze(uid, true).set_talent(uid, Talent::Precision, 120);
        spawn_pass(&mut a, &input);
        let mut expected = before;
        expected.set(Talent::Precision, 100);
        assert_eq!(expected, *a.entities().get(uid).unwrap().talents().unwrap());
        run(&mut a, 100);
        assert_eq!(expected, *a.entities().get(uid).unwrap().talents().unwrap());

        input.clear();
        input.set_talent(uid, Talent::Precision, -5);
        process_game_state(&mut a, 1.0 / 60.0, &input);
        assert_eq!(a.entities().get(uid).unwrap().talents().unwrap().get(Talent::Precision), 0);
    }

    #[test]
    fn a_talent_change_for_a_dog_or_missing_entity_is_refused() {
        let mut state = world();
        let dog = state.spawn(EntityType::Dog, Point::new(4, 4));
        assert!(state.entities().get(dog).unwrap().talents().is_none());
        assert!(!state.set_talent(dog, Talent::Arts, 80));
        assert!(!state.set_talent(Uid::new(EntityType::Human, 99), Talent::Arts, 80));
        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("has no talents")), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("no such entity")), "{lines:?}");
    }

    #[test]
    fn switching_a_process_off_holds_its_stat_and_switching_it_on_lets_it_move_again() {
        let mut state = world();
        let uid = state.spawn(EntityType::Human, Point::new(4, 4));
        run(&mut state, 1);

        let mut off = Input::new();
        off.set_process(uid, ProcessId::Hunger, false);
        process_game_state(&mut state, 1.0 / 60.0, &off);
        let held = hunger_of(&state, uid);
        run(&mut state, 300);
        assert_eq!(hunger_of(&state, uid), held);
        let biology = *state.entities().get(uid).unwrap().biology().unwrap();
        assert!(!biology.is_running(ProcessId::Hunger));
        assert!(biology.is_running(ProcessId::Thirst), "only the one asked for");

        let mut on = Input::new();
        on.set_process(uid, ProcessId::Hunger, true);
        process_game_state(&mut state, 1.0 / 60.0, &on);
        run(&mut state, 300);
        assert!(hunger_of(&state, uid) > held || held == 100.0);
    }

    #[test]
    fn switching_a_process_on_something_without_a_body_says_so_rather_than_panicking() {
        let mut state = world();
        let dog = state.spawn(EntityType::Dog, Point::new(4, 4));
        let stranger = Uid::new(EntityType::Human, 99);

        let mut input = Input::new();
        input.set_process(dog, ProcessId::Bladder, false);
        input.set_process(stranger, ProcessId::Bladder, false);
        spawn_pass(&mut state, &input);

        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("has no biology")), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("no such entity")), "{lines:?}");
    }

    #[test]
    fn freezing_something_that_is_not_there_says_so_rather_than_panicking() {
        let mut state = world();
        let stranger = Uid::new(EntityType::Human, 99);

        let mut input = Input::new();
        input.freeze(stranger, true);
        spawn_pass(&mut state, &input);

        assert!(!state.set_frozen(stranger, true));
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

    /// The time scale, seen from the outside: a second of watching is two
    /// minutes of world, and the world's clock is what says so.
    #[test]
    fn a_second_of_watching_moves_the_world_clock_two_minutes() {
        let mut state = world();
        assert_eq!(state.clock().time(), "08:00:00");

        run(&mut state, 60);
        assert_eq!(state.clock().time(), "08:02:00");
        assert_eq!(state.clock().day(), 1);

        // Half an hour of world, which is fifteen seconds of watching.
        run(&mut state, 14 * 60);
        assert_eq!(state.clock().time(), "08:30:00");
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

    /// The `count` dry cells nearest `near`, for spawning out in the
    /// wasteland, where a given cell might be a lake.
    /// The `count` cells nearest `near` that are open wasteland, never a
    /// pocket of dry ground a lake has walled in: from there nothing is
    /// reachable, which is the right answer and not what these tests are about.
    fn dry_cells(map: &Map, near: Point, count: usize) -> Vec<Point> {
        let mut cells: Vec<Point> = crate::map::Bounds::from_corners(near.offset(-20, -20), near.offset(20, 20))
            .points()
            .filter(|&cell| map.is_passable(cell) && map.navigation().is_outside(cell))
            .collect();
        cells.sort_by_key(|cell| (cell.manhattan_distance(near), *cell));
        cells.truncate(count);
        assert_eq!(cells.len(), count, "dry ground near {near:?}");
        cells
    }

    #[test]
    fn a_crowd_out_in_the_wasteland_walks_it_and_the_ground_loads_round_them() {
        let build = || {
            let map = Map::new(Size::new(8, 8), FLOOR).with_wasteland(5);
            let far = Point::new(-700, 450);
            let cells = dry_cells(&map, far, 12);
            let mut state = GameState::new(map, 3);
            assert!(!state.map.is_chunk_loaded(crate::map::ChunkPos::of(far)), "nobody has been out there");
            for (i, &cell) in cells.iter().enumerate() {
                state.spawn(if i % 2 == 0 { EntityType::Dog } else { EntityType::Human }, cell);
            }
            (state, cells)
        };
        let (mut a, start) = build();
        run(&mut a, 1500);
        let mut moved = 0;
        for (entity, &from) in a.entities().iter().zip(&start) {
            let here = entity.center_position();
            assert!(a.map.is_passable(here), "{here:?} is not ground");
            assert!(!a.map.contains(here), "nobody walked the 700 cells back");
            assert_eq!(a.occupancy().occupant(here), Some(entity.uid()), "{here:?}");
            for pos in crate::map::ChunkPos::of(here).around(1) {
                assert!(a.map.is_chunk_loaded(pos) || a.map.site().contains(pos.origin()), "{pos:?} round {here:?}");
            }
            moved += usize::from(here != from);
        }
        assert!(moved > start.len() / 2, "only {moved} of {} moved", start.len());
        census_agrees(&a);

        let (mut b, _) = build();
        run(&mut b, 1500);
        let positions = |state: &GameState| -> Vec<(u64, (f32, f32))> {
            state.entities().iter().map(|e| (e.uid().raw(), e.position())).collect()
        };
        assert_eq!(positions(&a), positions(&b));
    }

    #[test]
    fn what_is_loaded_never_changes_what_happens() {
        // The same world twice, one with the wasteland loaded far and wide
        // before anybody moves and one loading it as the crowd gets there:
        // a chunk is a cache, so the two must not differ by a single step.
        let build = |preload: bool| {
            let mut map = Map::new(Size::new(10, 10), FLOOR).with_wasteland(9);
            if preload {
                map.load_chunks(crate::map::Bounds::from_corners(Point::new(-200, -200), Point::new(200, 200)));
            }
            let cells = dry_cells(&map, Point::new(-30, 5), 10);
            let mut state = GameState::new(map, 4);
            for &cell in &cells {
                state.spawn(EntityType::Dog, cell);
            }
            state
        };
        let (mut cold, mut warm) = (build(false), build(true));
        assert!(warm.map.loaded_chunks() > cold.map.loaded_chunks());
        run(&mut cold, 800);
        run(&mut warm, 800);
        let positions = |state: &GameState| -> Vec<(u64, (f32, f32))> {
            state.entities().iter().map(|e| (e.uid().raw(), e.position())).collect()
        };
        assert_eq!(positions(&cold), positions(&warm));
    }

    /// The census has everybody, in the chunk each one stands in, and
    /// nobody twice.
    fn census_agrees(state: &GameState) {
        let mut filed = 0;
        for (slot, entity) in state.entities().iter_slots() {
            let pos = crate::map::ChunkPos::of(entity.center_position());
            assert_eq!(state.census().chunk_of(slot), Some(pos), "{} at {:?}", entity.uid(), entity.center_position());
            assert!(state.census().units_in(pos).any(|filed| filed == slot));
        }
        for pos in state.census().occupied() {
            filed += state.census().count(pos);
        }
        assert_eq!(filed, state.entities().len());
    }

    #[test]
    fn the_wasteland_behind_a_crowd_is_let_go_once_it_has_gone() {
        let map = Map::new(Size::new(10, 10), FLOOR).with_wasteland(5);
        let cells = dry_cells(&map, Point::new(-700, 450), 6);
        let mut state = GameState::new(map, 3);
        let site_only = state.map.loaded_chunks();
        let crowd: Vec<Uid> = cells.iter().map(|&cell| state.spawn(EntityType::Dog, cell)).collect();
        run(&mut state, 60);
        assert!(state.map.loaded_chunks() >= site_only + 9, "the ground round them is loaded");
        assert!(state.wild_chunks() >= 9 && state.census().lists() >= 9);
        census_agrees(&state);

        let mut input = Input::new();
        for &uid in &crowd {
            input.despawn(uid);
        }
        process_game_state(&mut state, 1.0 / 60.0, &input);
        // A few passes of the bounded sweep, and everything out there goes
        // — but never the navigation area round the site.
        run(&mut state, 20);
        assert_eq!(state.map.loaded_chunks(), site_only);
        assert_eq!(state.wild_chunks(), 0);
        assert_eq!(state.census().lists(), 0);
    }

    #[test]
    fn reclaiming_chunks_never_changes_what_happens() {
        // Two worlds with the wasteland loaded far and wide, a crowd walking
        // out in it: one lets go of what nobody is near as it goes, the
        // other keeps everything. Not a step between them may differ.
        let build = |reclaim: bool| {
            let mut map = Map::new(Size::new(10, 10), FLOOR).with_wasteland(9);
            map.load_chunks(crate::map::Bounds::from_corners(Point::new(-400, -300), Point::new(300, 300)));
            let cells = dry_cells(&map, Point::new(-250, 120), 16);
            let mut state = GameState::new(map, 4);
            if !reclaim {
                state.keep_every_chunk();
            }
            for (i, &cell) in cells.iter().enumerate() {
                state.spawn(if i % 3 == 0 { EntityType::Human } else { EntityType::Dog }, cell);
            }
            state
        };
        let (mut thrifty, mut hoarder) = (build(true), build(false));
        run(&mut thrifty, 1200);
        run(&mut hoarder, 1200);
        assert!(
            thrifty.map.loaded_chunks() * 4 < hoarder.map.loaded_chunks(),
            "{} chunks kept against {}",
            thrifty.map.loaded_chunks(),
            hoarder.map.loaded_chunks()
        );
        let state_of = |state: &GameState| -> Vec<(u64, (f32, f32), Option<GoalId>)> {
            state.entities().iter().map(|e| (e.uid().raw(), e.position(), e.current_goal())).collect()
        };
        assert_eq!(state_of(&thrifty), state_of(&hoarder));
        census_agrees(&thrifty);
    }

    /// A hundred and twenty-eight hungry people on one side of a long wall,
    /// the fridge on the other and the only gap at the far end: each route
    /// is a search of most of a side, thousands of cells, and they all ask on
    /// the same tick. Unbudgeted that was the whole lot in one tick.
    fn a_hungry_crowd_behind_a_wall() -> (GameState, Vec<(Uid, Point)>) {
        let mut map = Map::new(Size::new(96, 96), FLOOR);
        for y in 0..95 {
            map.set_terrain(Point::new(48, y), WALL);
        }
        crate::sim::testing::prop_at(&mut map, "fridge", Point::new(90, 2));
        crate::map::utilities::serve_everything(&mut map, Point::new(95, 95), Point::new(94, 95));
        let mut state = GameState::new(map, 11);
        let mut crowd = Vec::new();
        for y in 2..10 {
            for x in 2..18 {
                let at = Point::new(x, y);
                let uid = state.spawn(EntityType::Human, at);
                state.entities.get_mut(uid).unwrap().biology_mut().unwrap().edit(|_| {
                    crate::sim::biology::Stats::calm()
                        .with_hunger(95.0)
                        .with_thirst(0.0)
                        .with_fun(100.0)
                        .with_stamina(100.0)
                        .with_bladder(0.0)
                });
                crowd.push((uid, at));
            }
        }
        (state, crowd)
    }

    #[test]
    fn the_route_budget_bounds_a_tick_and_everybody_gets_their_route_in_turn() {
        let (mut state, crowd) = a_hungry_crowd_behind_a_wall();
        let mut busiest = 0;
        let mut waited = false;
        let mut routed = std::collections::HashSet::new();
        for tick in 0..400 {
            run(&mut state, 1);
            busiest = busiest.max(state.route_work());
            assert!(state.route_work() <= ROUTE_BUDGET, "tick {tick}: {} cells", state.route_work());
            // Never more unfinished searches held than the blocks may keep.
            let mut held = [0usize; 2];
            for (slot, entity) in state.entities().iter_slots() {
                held[slot as usize / ROUTE_BLOCK] += usize::from(entity.walker().unwrap().is_searching());
            }
            assert!(held.iter().all(|&held| held <= SEARCHES_KEPT), "tick {tick}: {held:?}");
            // Somebody who has asked for a route and has none yet is waiting
            // its turn.
            for entity in state.entities().iter() {
                let walker = entity.walker().unwrap();
                if walker.is_walking() && walker.path_cells().is_empty() {
                    waited = true;
                } else if walker.path_cells().len() > 50 {
                    routed.insert(entity.uid());
                }
            }
        }
        assert!(busiest > ROUTE_BUDGET / 2, "the crowd should have wanted more than the budget: {busiest}");
        assert!(waited, "nobody had to wait, so the budget never bit");
        // ...and nobody waited for ever: every one of them was handed its
        // route round the wall. (Walking it is another matter — a hundred
        // and twenty-eight people through one gap is a jam.)
        for (uid, _) in crowd {
            assert!(routed.contains(&uid), "{uid} never got its route");
        }
    }

    #[test]
    fn a_block_holds_no_more_unfinished_searches_than_it_may() {
        // Routes longer than one walker may search in a tick (`FAR_LIMIT`):
        // the far side of a wall across a 400-cell map. The first to ask
        // keeps its unfinished search; the rest of the block get a trial and
        // wait their turn, rather than each holding megabytes of scratch.
        let mut map = Map::new(Size::new(400, 400), FLOOR);
        for y in 0..399 {
            map.set_terrain(Point::new(200, y), WALL);
        }
        crate::sim::testing::prop_at(&mut map, "fridge", Point::new(390, 2));
        crate::map::utilities::serve_everything(&mut map, Point::new(399, 399), Point::new(398, 399));
        let mut state = GameState::new(map, 5);
        for x in 2..10 {
            let uid = state.spawn(EntityType::Human, Point::new(x, 2));
            state.entities.get_mut(uid).unwrap().biology_mut().unwrap().edit(|_| {
                crate::sim::biology::Stats::calm().with_hunger(95.0).with_thirst(0.0).with_fun(100.0).with_stamina(100.0).with_bladder(0.0)
            });
        }
        let mut most = 0;
        for tick in 0..4 {
            run(&mut state, 1);
            let held = state.entities().iter().filter(|e| e.walker().unwrap().is_searching()).count();
            assert!(held <= SEARCHES_KEPT, "tick {tick}: {held} unfinished searches held");
            most = most.max(held);
        }
        assert_eq!(most, SEARCHES_KEPT, "the routes were long enough that one was kept");
    }

    #[test]
    fn a_budgeted_crowd_is_the_same_crowd_every_run() {
        // Above `PARALLEL_AT`, so the blocks react on threads: who gets the
        // budget must not depend on which thread got there first.
        let (mut a, _) = a_hungry_crowd_behind_a_wall();
        let (mut b, _) = a_hungry_crowd_behind_a_wall();
        run(&mut a, 300);
        run(&mut b, 300);
        let positions = |state: &GameState| -> Vec<(u64, (f32, f32))> {
            state.entities().iter().map(|e| (e.uid().raw(), e.position())).collect()
        };
        assert_eq!(positions(&a), positions(&b));
    }

    #[test]
    fn a_wanderer_shut_in_at_the_edge_of_the_site_still_walks_its_room() {
        // A sealed 5x3 room in the site's corner, the wasteland just past its
        // walls: most of the square a wanderer picks from is ground it cannot
        // reach. Picking those anyway is a failed route each time, and a
        // failed route holds wandering off, so it would mostly stand still.
        let mut map = Map::new(Size::new(8, 6), WALL);
        for cell in crate::map::Bounds::from_corners(Point::new(1, 1), Point::new(5, 3)).points() {
            map.set_terrain(cell, FLOOR);
        }
        let mut state = GameState::new(map.with_wasteland(3), 7);
        let uid = state.spawn(EntityType::Human, Point::new(1, 1));
        state.entities.get_mut(uid).unwrap().biology_mut().unwrap().edit(|_| crate::sim::biology::Stats::calm());
        let mut visited = std::collections::HashSet::new();
        for _ in 0..2000 {
            run(&mut state, 1);
            visited.insert(state.entities().get(uid).unwrap().center_position());
        }
        assert!(visited.len() >= 10, "walked only {} of the room's 15 cells: {visited:?}", visited.len());
    }

    #[test]
    fn a_hungry_human_out_in_the_wasteland_walks_home_to_the_fridge() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        crate::sim::testing::prop_at(&mut map, "fridge", Point::new(6, 6));
        crate::map::utilities::serve_everything(&mut map, Point::new(0, 0), Point::new(11, 0));
        let map = map.with_wasteland(2);
        let out = dry_cells(&map, Point::new(-45, 20), 1)[0];
        let mut state = GameState::new(map, 7);
        let uid = state.spawn(EntityType::Human, out);
        state.entities.get_mut(uid).unwrap().biology_mut().unwrap().edit(|_| {
            crate::sim::biology::Stats::calm()
                .with_hunger(95.0)
                .with_thirst(0.0)
                .with_fun(100.0)
                .with_stamina(100.0)
                .with_bladder(0.0)
        });
        let mut lines = Vec::new();
        for _ in 0..6000 {
            run(&mut state, 1);
            lines.extend(state.log.drain());
            if lines.iter().any(|line| line.contains("ate at the fridge at 6, 6")) {
                return;
            }
        }
        let tail: Vec<&String> = lines.iter().rev().take(25).collect();
        panic!("never got back to eat; ended at {:?}; {tail:#?}", state.entities().get(uid).unwrap().center_position());
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

    /// A map with fridges, toilets and computers in it, so a crowd has every
    /// goal to take turns between rather than one.
    fn kitchen(size: i32, seed: u64) -> GameState {
        let mut map = crate::sim::testing::home_map(Size::new(size, size));
        let props = [
            (Point::new(2, 2), "fridge"),
            (Point::new(size - 3, size - 3), "fridge"),
            (Point::new(size - 3, 2), "toilet"),
            (Point::new(2, size - 3), "toilet"),
            (Point::new(size / 2, 2), "computer"),
            (Point::new(size / 2, size - 3), "computer"),
        ];
        for (cell, kind) in props {
            crate::sim::testing::prop_at(&mut map, kind, cell);
        }
        // Plumbed and wired, from two corners nobody needs.
        crate::map::utilities::serve_everything(&mut map, Point::new(0, 0), Point::new(size - 1, 0));
        GameState::new(map, seed)
    }

    #[test]
    fn determinism_survives_brains_and_biology_in_the_parallel_rounds() {
        // The same check as above with every part of the brain running: goals
        // handing over, eating, drinking, the toilet and the computer
        // competing for the top,
        // routes to fridges and toilets, a queue built and resumed — across
        // the threshold where react goes onto rayon. Iteration order, routine
        // order and tie-breaking are all properties of the types; this is the
        // test that says they add up.
        let build = || {
            let mut state = kitchen(24, 17);
            for i in 0..(PARALLEL_AT as i32 + 20) {
                let uid = state.spawn(EntityType::Human, Point::new(1 + i % 22, 4 + (i / 22) % 16));
                // A shared home: its computers can be used for gaming as
                // well as work, so both behaviours run in this test.
                state.entities.get_mut(uid).unwrap().body_mut().set_home(PropertyId::new(1));
                state.hire(uid);
            }
            state
        };
        let (mut a, mut b) = (build(), build());
        let count = |lines: &[String], needle: &str| lines.iter().filter(|l| l.contains(needle)).count();
        let tally = |lines: &[String]| {
            [
                count(lines, "ate at the fridge"),
                count(lines, "drank at the fridge"),
                count(lines, "used the toilet"),
                count(lines, "had a go on the computer"),
                count(lines, "opened the fridge"),
                count(lines, "worked an hour at the computer"),
                count(lines, " chatted with "),
                count(lines, " made small talk with "),
            ]
        };
        let (mut done_a, mut done_b) = ([0; 8], [0; 8]);
        // Everybody is born 70 to 100 percent satisfied, so nobody is hungry,
        // parched, bursting or bored for hours: the slowest need to come round
        // is boredom, ten world hours from a great time to bored. `run` ticks
        // at 60Hz, so a world hour is 1800 of them, and this is six.
        for _ in 0..(6 * 1800) {
            run(&mut a, 1);
            run(&mut b, 1);
            for (total, now) in done_a.iter_mut().zip(tally(&a.log.drain())) {
                *total += now;
            }
            for (total, now) in done_b.iter_mut().zip(tally(&b.log.drain())) {
                *total += now;
            }
        }

        let positions = |state: &GameState| -> Vec<(u64, (f32, f32))> {
            state
                .entities()
                .iter()
                .map(|e| (e.uid().raw(), e.position()))
                .collect()
        };
        let [meals, drinks, reliefs, plays, opened, worked, chats, smalltalks] = done_a;
        assert!(meals > 0, "nobody ate, so the eating part of the brain never ran");
        assert!(drinks > 0, "nobody drank, so the drinking part of the brain never ran");
        assert!(reliefs > 0, "nobody used a toilet, so that part of the brain never ran");
        assert!(plays > 0, "nobody had a go on a computer, so that part of the brain never ran");
        assert!(opened > 0, "nobody opened a fridge, so the world step never applied an effect");
        // The first two arrivals were given the two computers as desks, and
        // from eight to two on a Monday they worked.
        assert!(worked > 0, "nobody worked a shift, so the work part of the brain never ran");
        // Wanderers who met, all family under one roof, and chatted.
        assert!(chats > 0, "nobody chatted, so the chat part of the brain never ran");
        assert!(smalltalks > 0, "nobody made small talk, so that part of the brain never ran");
        assert_eq!(a.workplaces().len(), 2);
        assert_eq!(done_a, done_b);
        assert_eq!(positions(&a), positions(&b));
        // Looking round and meeting people, which changes nothing a decision
        // reads yet and so cannot show in a position: what everybody's day
        // came to, and what they remember of it, has to agree as well.
        let mood = |state: &GameState| -> Vec<(f32, f32)> {
            state
                .entities()
                .iter()
                .filter_map(|e| e.biology())
                .map(|b| {
                    let company = b.recollection().familiarity(biology::Experience::Company);
                    (b.stats().satisfaction(), company)
                })
                .collect()
        };
        assert!(mood(&a).iter().any(|&(_, company)| company > 0.0), "nobody met anybody");
        assert_eq!(mood(&a), mood(&b));
        // The world step, across the same threshold: two fridges' doors and
        // temperatures, settled by the same slot order on every run.
        assert_eq!(
            a.fridges().iter().collect::<Vec<_>>(),
            b.fridges().iter().collect::<Vec<_>>()
        );
    }

    /// A world with a bed in each of `beds` and nothing else in it.
    fn with_beds(beds: &[Point], seed: u64) -> GameState {
        let mut map = Map::new(Size::new(24, 17), FLOOR);
        for (i, cell) in beds.iter().enumerate() {
            map.add_object(
                crate::map::ObjectLayer::Props,
                crate::map::Object {
                    at: Point::new(
                        cell.x * crate::map::PIXELS_PER_CELL + 24,
                        cell.y * crate::map::PIXELS_PER_CELL + 24,
                    ),
                    kind: crate::map::ObjectKind::new(&format!("bed {}", 1 + i % 6)),
                },
            );
        }
        GameState::new(map, seed)
    }

    /// A dormitory: fewer beds than people, so most of a crowd has no bed of
    /// its own.
    fn dormitory(seed: u64) -> GameState {
        let beds: Vec<Point> = (0..12)
            .map(|i| Point::new(2 + (i % 6) * 3, if i < 6 { 2 } else { 14 }))
            .collect();
        with_beds(&beds, seed)
    }

    #[test]
    fn a_human_who_arrives_is_given_the_nearest_bed_nobody_owns() {
        let (near, far) = (Point::new(2, 2), Point::new(20, 14));
        let mut state = with_beds(&[near, far], 5);

        let first = state.spawn(EntityType::Human, Point::new(4, 4));
        assert_eq!(state.homes().owner(near), Some(first), "the nearer of the two");
        assert!(state.log.drain().iter().any(|line| line.contains("was given the bed at 2, 2")));

        // Arriving in the same place, the nearer bed is taken: **only a bed
        // that is free is given away**, so the second one gets the other.
        let second = state.spawn(EntityType::Human, Point::new(4, 4));
        assert_eq!(state.homes().owner(far), Some(second));
        assert_eq!(state.homes().owner(near), Some(first), "and the first keeps theirs");

        // Every bed is somebody's now, and a third arrival has none.
        state.spawn(EntityType::Human, Point::new(4, 4));
        assert_eq!(state.homes().len(), 2);
    }

    #[test]
    fn what_a_human_remembers_is_which_bed_is_its_own() {
        let mut state = with_beds(&[Point::new(6, 9)], 5);
        let human = state.spawn(EntityType::Human, Point::new(1, 1));
        let fields = state.entities().get(human).expect("just spawned").brain_fields();
        let memory = fields.iter().find(|(name, _)| *name == "memory").expect("a human has one");
        assert!(memory.1.contains("home bed 6, 9"), "memory says {:?}", memory.1);
    }

    #[test]
    fn a_dog_is_not_given_a_bed_and_does_not_use_one_up() {
        let bed = Point::new(2, 2);
        let mut state = with_beds(&[bed], 5);
        state.spawn(EntityType::Dog, Point::new(3, 3));
        assert!(state.homes().is_empty());

        let human = state.spawn(EntityType::Human, Point::new(3, 3));
        assert_eq!(state.homes().owner(bed), Some(human), "still there for the person");
    }

    #[test]
    fn a_world_with_no_beds_gives_nobody_one() {
        let mut state = with_beds(&[], 5);
        state.spawn(EntityType::Human, Point::new(3, 3));
        assert!(state.homes().is_empty());
    }

    #[test]
    fn leaving_gives_the_bed_back_to_be_given_to_the_next_arrival() {
        let bed = Point::new(2, 2);
        let mut state = with_beds(&[bed], 5);
        let first = state.spawn(EntityType::Human, Point::new(3, 3));
        let stranger = state.spawn(EntityType::Human, Point::new(4, 3));
        assert_eq!(state.homes().owner(bed), Some(first));
        assert!(state.despawn(stranger), "one who owned nothing changes nothing");
        assert_eq!(state.homes().owner(bed), Some(first));

        assert!(state.despawn(first));
        assert!(!state.homes().is_owned(bed));
        let next = state.spawn(EntityType::Human, Point::new(3, 3));
        assert_eq!(state.homes().owner(bed), Some(next));
    }

    /// A world with a working computer in each of `desks` and nothing else.
    fn with_desks(desks: &[Point], seed: u64) -> GameState {
        let mut map = Map::new(Size::new(24, 17), FLOOR);
        for &desk in desks {
            crate::sim::testing::prop_at(&mut map, "computer", desk);
        }
        crate::map::utilities::serve_everything(&mut map, Point::new(0, 16), Point::new(23, 16));
        GameState::new(map, seed)
    }

    #[test]
    fn an_arrival_is_hired_at_the_nearest_free_computer_until_there_are_none() {
        let (near, far) = (Point::new(2, 2), Point::new(20, 12));
        let mut state = with_desks(&[near, far], 5);
        let first = state.spawn(EntityType::Human, Point::new(4, 4));
        assert_eq!(state.workplaces().worker(near, job::OFFICE_HOURS), Some(first), "the nearer of the two");
        let job = state.entities().get(first).unwrap().job().expect("hired");
        assert_eq!((job.kind, job.workplace), (JobKind::OfficeWorker, near));
        assert!(state.log.drain().iter().any(|line| line.contains("was hired as an office worker at the computer at 2, 2")));

        let second = state.spawn(EntityType::Human, Point::new(4, 4));
        assert_eq!(state.workplaces().worker(far, job::OFFICE_HOURS), Some(second), "only a free desk is given");

        let third = state.spawn(EntityType::Human, Point::new(4, 4));
        assert_eq!(state.entities().get(third).unwrap().job(), None, "every desk taken: unemployed");
        assert_eq!(state.workplaces().len(), 2);
    }

    #[test]
    fn a_dog_is_not_hired_and_does_not_use_a_desk_up() {
        let desk = Point::new(2, 2);
        let mut state = with_desks(&[desk], 5);
        state.spawn(EntityType::Dog, Point::new(3, 3));
        assert!(state.workplaces().is_empty());
        let human = state.spawn(EntityType::Human, Point::new(3, 3));
        assert_eq!(state.workplaces().worker(desk, job::OFFICE_HOURS), Some(human));
    }

    #[test]
    fn leaving_gives_the_desk_back_to_be_given_to_the_next_arrival() {
        let desk = Point::new(2, 2);
        let mut state = with_desks(&[desk], 5);
        let first = state.spawn(EntityType::Human, Point::new(3, 3));
        assert!(state.despawn(first));
        assert!(!state.workplaces().is_taken(desk, job::OFFICE_HOURS));
        let next = state.spawn(EntityType::Human, Point::new(3, 3));
        assert_eq!(state.workplaces().worker(desk, job::OFFICE_HOURS), Some(next));
    }

    /// The two commands the job menu sends: let somebody go, and the desk is
    /// free; hire them back, and they have it again — the free desk nearest
    /// where they stand.
    #[test]
    fn a_unit_can_be_let_go_and_hired_again_from_outside_the_world() {
        let desk = Point::new(2, 2);
        let mut state = with_desks(&[desk], 5);
        let worker = state.spawn(EntityType::Human, Point::new(3, 3));
        let dog = state.spawn(EntityType::Dog, Point::new(5, 5));
        state.log.drain();

        process_game_state(&mut state, 1.0 / 60.0, Input::new().dismiss(worker));
        assert_eq!(state.entities().get(worker).unwrap().job(), None);
        assert!(state.workplaces().is_empty());
        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("was let go from the computer at 2, 2")), "{lines:?}");

        process_game_state(&mut state, 1.0 / 60.0, Input::new().dismiss(worker).hire(dog));
        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("has no job")), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("no workplace free")), "a dog cannot work: {lines:?}");
        assert!(state.workplaces().is_empty());

        process_game_state(&mut state, 1.0 / 60.0, Input::new().hire(worker).hire(worker));
        assert_eq!(state.workplaces().worker(desk, job::OFFICE_HOURS), Some(worker));
        assert_eq!(state.entities().get(worker).unwrap().job().map(|job| job.workplace), Some(desk));
        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("is already an office worker")), "{lines:?}");
    }

    /// A world with a working computer in each of `desks` and a scrap bin in
    /// each of `bins`.
    fn with_desks_and_bins(desks: &[Point], bins: &[Point], seed: u64) -> GameState {
        let mut map = Map::new(Size::new(24, 17), FLOOR);
        for &desk in desks {
            crate::sim::testing::prop_at(&mut map, "computer", desk);
        }
        for &bin in bins {
            crate::sim::testing::prop_at(&mut map, "scrap bin", bin);
        }
        crate::map::utilities::serve_everything(&mut map, Point::new(0, 16), Point::new(23, 16));
        GameState::new(map, seed)
    }

    /// The whole chain on a Monday: the courier takes the warehouse's scrap
    /// to the factory's hopper, the steelworker takes it from there, smelts
    /// it at its furnace and puts the steel on the pallet, and the courier
    /// takes the steel back to the warehouse's rack. Two pieces of scrap to
    /// an ingot, and nothing made or lost on the way.
    #[test]
    fn a_courier_and_a_steelworker_turn_the_warehouse_s_scrap_into_steel_on_its_rack() {
        let (mut state, smelter, courier) = crate::sim::testing::steel_works(12, 3);
        let (bin, rack, hopper, pallet) = (Point::new(2, 2), Point::new(2, 8), Point::new(16, 2), Point::new(16, 8));
        let mut lines = Vec::new();
        let mut smelting = false;
        // 1800 ticks a world hour: by five o'clock at the latest.
        for _ in 0..9 * 1800 {
            run(&mut state, 1);
            lines.extend(state.log.drain());
            smelting |= state.entities().get(smelter).unwrap().interacting_with() == Some(Point::new(21, 5));
            if state.bins().stored(rack) >= 2 {
                break;
            }
        }
        let tail = &lines[lines.len().saturating_sub(30)..];
        assert!(state.bins().stored(rack) >= 2, "{} steel on the rack by {}; {tail:?}", state.bins().stored(rack), state.clock());
        assert!(smelting, "the steelworker was at its furnace");
        assert!(lines.iter().any(|line| line.contains(&format!("{courier} took 3 scrap metal from the bin at 2, 2"))), "{tail:?}");
        assert!(lines.iter().any(|line| line.contains(&format!("{courier} stored 3 scrap metal in the scrap hopper at 16, 2"))), "{tail:?}");
        assert!(lines.iter().any(|line| line.contains(&format!("{smelter} took")) && line.contains("from the scrap hopper")), "{tail:?}");
        assert!(lines.iter().any(|line| line.contains(&format!("{smelter} stored 1 steel in the steel pallet at 16, 8"))), "{tail:?}");
        assert!(lines.iter().any(|line| line.contains(&format!("{courier} took")) && line.contains("steel from the steel pallet")), "{tail:?}");

        // Every piece of scrap is somewhere: in a store, in a pack, or two
        // of them in every ingot.
        let carried = |uid: Uid, item: ItemKind| state.entities().get(uid).unwrap().inventory().unwrap().count(item) as u32;
        let scrap = state.bins().stored(bin) + state.bins().stored(hopper) + carried(smelter, ItemKind::Scrap) + carried(courier, ItemKind::Scrap);
        let steel = state.bins().stored(pallet) + state.bins().stored(rack) + carried(smelter, ItemKind::Steel) + carried(courier, ItemKind::Steel);
        assert_eq!(scrap + 2 * steel, 12, "{scrap} scrap and {steel} steel");
        let stats = state.statistics();
        assert_eq!((stats.stock("warehouse", ItemKind::Steel), stats.stock("factory", ItemKind::Steel)), (state.bins().stored(rack), state.bins().stored(pallet)));
    }

    /// The desks are offered first, however near the bins are; then one
    /// collector to a bin; then nobody. `Command::Hire` offers them in the
    /// same order.
    #[test]
    fn an_arrival_is_offered_a_desk_before_a_scrap_bin_and_a_bin_is_one_collectors() {
        let desks = [Point::new(20, 12), Point::new(22, 12)];
        let bins = [Point::new(2, 2), Point::new(4, 2)];
        let mut state = with_desks_and_bins(&desks, &bins, 5);
        let arrivals: Vec<Uid> = (0..5).map(|_| state.spawn(EntityType::Human, Point::new(3, 3))).collect();
        let job = |state: &GameState, uid: Uid| state.entities().get(uid).unwrap().job();

        for &uid in &arrivals[..2] {
            let job = job(&state, uid).expect("hired");
            assert_eq!(job.kind, JobKind::OfficeWorker, "a desk first, though the bins are beside it");
            assert!(desks.contains(&job.workplace));
        }
        for &uid in &arrivals[2..4] {
            let job = job(&state, uid).expect("hired");
            assert_eq!(job.kind, JobKind::ScrapCollector);
            assert!(bins.contains(&job.workplace));
        }
        assert_eq!(job(&state, arrivals[3]).unwrap().workplace, bins[1], "the bin nobody had");
        assert_eq!(job(&state, arrivals[4]), None, "every workplace taken: unemployed");
        assert_eq!(state.workplaces().len(), 4);
        let stats = state.statistics();
        let counts: Vec<(JobKind, u32, u32)> = stats.jobs.iter().map(|job| (job.kind, job.workers, job.posts)).collect();
        assert_eq!(counts[..2], [(JobKind::OfficeWorker, 2, 2), (JobKind::ScrapCollector, 2, 2)]);
        assert_eq!((stats.living(), stats.jobless, stats.stock("warehouse", ItemKind::Scrap), stats.has("warehouse"), stats.has("factory")), (5, 1, 0, true, false));
        let lines = state.log.drain();
        assert!(
            lines.iter().any(|line| line.contains("was hired as a scrap collector at the scrap bin at 2, 2")),
            "{lines:?}"
        );

        // A desk and a bin free again: hiring gives the desk.
        let (clerk, collector) = (arrivals[0], arrivals[2]);
        process_game_state(&mut state, 1.0 / 60.0, Input::new().dismiss(clerk).dismiss(collector));
        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("was let go from the scrap bin at 2, 2")), "{lines:?}");
        process_game_state(&mut state, 1.0 / 60.0, Input::new().hire(collector).hire(arrivals[4]).hire(arrivals[3]));
        assert_eq!(job(&state, collector).map(|job| job.kind), Some(JobKind::OfficeWorker), "the office first");
        assert_eq!(job(&state, arrivals[4]).map(|job| job.kind), Some(JobKind::ScrapCollector));
        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("is already a scrap collector")), "{lines:?}");
    }

    /// A gate kept round the clock: three arrivals hired to one guard post,
    /// a watch each in hiring order, each with a bed of its own and
    /// everything but tiredness switched off. Sampled every quarter of an
    /// hour for a day from the first watch, somebody is standing at the post
    /// at nearly every look — the changes of watch, when one walks off as
    /// the next walks up, are the gaps — and each of the three keeps its
    /// watch, sleeping at its own time.
    /// Somebody dies, and the undertaker on watch takes the body up, carries
    /// it to the crematorium and cremates it; the dead leave the world, the
    /// cell they lay in is nobody's, and a dead unit thinks no more.
    #[test]
    fn the_dead_are_carried_to_the_crematorium_and_leave_the_world() {
        let crematorium = Point::new(1, 1);
        let mut map = Map::new(Size::new(16, 10), FLOOR);
        crate::sim::testing::prop_at(&mut map, "crematorium", crematorium);
        let mut state = GameState::new(map, 3);
        let undertaker = state.spawn(EntityType::Human, Point::new(2, 1));
        let job = state.entities().get(undertaker).unwrap().job().expect("hired");
        assert_eq!((job.kind, job.shift.start()), (JobKind::Undertaker, 8));
        let victim = state.spawn(EntityType::Human, Point::new(13, 7));
        assert!(state.dismiss(victim));
        state
            .entity_mut(victim)
            .unwrap()
            .biology_mut()
            .unwrap()
            .edit(|stats| stats.with_health(0.5).with_hunger(100.0).with_thirst(100.0));

        let mut input = Input::new();
        for process in ProcessId::ALL {
            input.set_process(undertaker, process, false);
        }
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let mut lines = Vec::new();
        let mut lay_dead = false;
        for _ in 0..(3 * 1800) {
            run(&mut state, 1);
            lines.extend(state.log.drain());
            if let Some(body) = state.entities().get(victim) {
                if body.is_dead() {
                    lay_dead = true;
                    assert_eq!(body.current_goal(), None);
                    assert_eq!(state.occupancy.occupant(body.center_position()), None, "the dead hold no cell");
                    assert_eq!(state.corpses().len(), 1);
                }
            }
            if lines.iter().any(|line| line.contains("cremated a body")) {
                break;
            }
        }
        assert!(lay_dead);
        assert!(lines.iter().any(|line| line == &format!("{victim} died of thirst")), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains(&format!("{undertaker} took up the body of {victim}"))), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains("cremated a body in the crematorium at 1, 1")), "{lines:?}");
        assert!(state.entities().get(victim).is_none(), "the body left the world");
        assert!(state.corpses().is_empty());
        assert_eq!(state.entities().get(undertaker).unwrap().inventory().unwrap().hand(), None);

        // Gone from the world, and still counted.
        let stats = state.statistics();
        assert_eq!(stats.alive, vec![(EntityType::Human, 1), (EntityType::Dog, 0), (EntityType::Deathdog, 0)]);
        assert_eq!((stats.died, stats.lying, stats.carried, stats.cremated), (1, 0, 0, 1));
        let undertakers = stats.jobs.iter().find(|job| job.kind == JobKind::Undertaker).unwrap();
        assert_eq!((undertakers.workers, undertakers.posts), (1, 3), "one crematorium, three watches");
        assert_eq!(stats.jobless, 0);
    }

    #[test]
    fn a_guard_post_is_kept_round_the_clock_by_its_three_watches() {
        let post = Point::new(22, 8);
        let mut map = Map::new(Size::new(24, 17), FLOOR);
        for x in [2, 6, 10] {
            crate::sim::testing::prop_at(&mut map, "bed 1", Point::new(x, 14));
        }
        crate::sim::testing::prop_at(&mut map, "guard post", post);
        let mut state = GameState::new(map, 3);
        let guards: Vec<Uid> = [2, 6, 10].into_iter().map(|x| state.spawn(EntityType::Human, Point::new(x, 13))).collect();
        let watch = |state: &GameState, uid: Uid| state.entities().get(uid).unwrap().job().map(|job| (job.kind, job.shift.start()));
        let watches: Vec<_> = guards.iter().map(|&uid| watch(&state, uid)).collect();
        assert_eq!(watches, [Some((JobKind::Guard, 8)), Some((JobKind::Guard, 16)), Some((JobKind::Guard, 0))]);
        let lines = state.log.drain();
        assert!(lines.iter().any(|line| line.contains("was hired as a guard at the guard post at 22, 8, 00:00-08:00")), "{lines:?}");
        let fourth = state.spawn(EntityType::Human, Point::new(14, 13));
        assert_eq!(watch(&state, fourth), None, "three guards to a post");
        assert!(state.despawn(fourth));

        let mut input = Input::new();
        for &uid in &guards {
            for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
                input.set_process(uid, process, false);
            }
        }
        process_game_state(&mut state, 1.0 / 60.0, &input);
        // 1800 ticks a world hour at `run`'s 60Hz: to half past eight, then
        // a look every quarter of an hour for a day.
        run(&mut state, 900);
        let (mut looks, mut kept) = (0, 0);
        let mut watched = Vec::new();
        for _ in 0..96 {
            run(&mut state, 450);
            looks += 1;
            let at_post: Vec<Uid> = guards
                .iter()
                .copied()
                .filter(|&uid| state.entities().get(uid).unwrap().interacting_with() == Some(post))
                .collect();
            kept += usize::from(!at_post.is_empty());
            watched.extend(state.log.drain().into_iter().filter(|line| line.contains("kept watch an hour at the guard post")));
        }
        assert!(kept * 10 >= looks * 9, "the post kept at {kept} of {looks} looks, by {}", state.clock());
        for &uid in &guards {
            let hours = watched.iter().filter(|line| line.starts_with(&format!("{uid} "))).count();
            assert!(hours >= 5, "{uid} kept {hours} hours of its watch: {watched:?}");
        }
    }

    #[test]
    fn determinism_survives_a_crowd_going_to_bed_in_the_parallel_rounds() {
        // The kitchen test above never puts anybody in a bed. This one starts
        // at ten at night, with more people than beds and more people than the
        // parallel threshold: twelve owners heading for their own, and
        // everybody else held off (and, as the night wears on, critical and
        // picking at random from whatever is free). All of it has to come out
        // the same twice.
        let build = || {
            let mut state = dormitory(23);
            state.elapsed = 14.0 * crate::sim::clock::HOUR as f64 / crate::sim::clock::TIME_SCALE as f64;
            for i in 0..(PARALLEL_AT as i32 + 20) {
                state.spawn(EntityType::Human, Point::new(i % 24, 5 + (i / 24) % 8));
            }
            state
        };
        assert!(build().clock().is_night());
        assert!(build().len() > PARALLEL_AT, "should actually cross the line");
        assert_eq!(build().homes().len(), 12, "every bed has an owner, and the rest have none");

        let (mut a, mut b) = (build(), build());
        let (mut slept_a, mut slept_b) = (0, 0);
        let count = |lines: Vec<String>| lines.iter().filter(|l| l.contains("slept in the bed")).count();
        // Ten world hours at 60Hz is 18000 ticks: the whole of the night, and
        // long enough for the unowned to get critical.
        for _ in 0..(10 * 1800) {
            run(&mut a, 1);
            run(&mut b, 1);
            slept_a += count(a.log.drain());
            slept_b += count(b.log.drain());
        }

        let positions = |state: &GameState| -> Vec<(u64, (f32, f32))> {
            state
                .entities()
                .iter()
                .map(|e| (e.uid().raw(), e.position()))
                .collect()
        };
        assert!(slept_a > 0, "nobody slept, so the sleeping part of the brain never ran");
        assert_eq!(slept_a, slept_b);
        assert_eq!(positions(&a), positions(&b));
    }

    #[test]
    fn a_crowd_jammed_round_one_toilet_spreads_out_to_another_instead_of_waiting_on_it() {
        // A dozen bursting humans round the near toilet, which takes one at a
        // time; a second toilet across the room. Without a memory of where
        // they gave up, every one of them would go back to the nearest after
        // its cooldown and the far toilet would never be used.
        let build = || {
            let mut map = Map::new(Size::new(28, 12), FLOOR);
            crate::sim::testing::prop_at(&mut map, "toilet", Point::new(4, 6));
            crate::sim::testing::prop_at(&mut map, "toilet", Point::new(25, 6));
            crate::map::utilities::serve_everything(&mut map, Point::new(0, 0), Point::new(27, 0));
            let mut state = GameState::new(map, 7);
            let mut uids = Vec::new();
            for i in 0..12 {
                uids.push(state.spawn(EntityType::Human, Point::new(2 + i % 6, 4 + (i / 6) * 4)));
            }
            for uid in uids {
                let biology = state.entities.get_mut(uid).unwrap().biology_mut().unwrap();
                biology.edit(|_| {
                    crate::sim::biology::Stats::calm()
                        .with_hunger(0.0)
                        .with_thirst(0.0)
                        .with_fun(100.0)
                        .with_stamina(100.0)
                        .with_bladder(95.0)
                });
            }
            state
        };
        let mut state = build();
        let mut lines = Vec::new();
        for _ in 0..3000 {
            run(&mut state, 1);
            lines.extend(state.log.drain());
        }
        let count = |needle: &str| lines.iter().filter(|l| l.contains(needle)).count();
        assert!(count("gave up on reaching 4, 6") > 0, "nobody gave up on the near toilet");
        assert!(count("used the toilet at 25, 6") > 0, "nobody went on to the far toilet");
        assert!(count("used the toilet at 4, 6") > 0, "and the near one still got used");

        // ...and the same twice.
        let mut again = build();
        run(&mut again, 3000);
        let positions = |state: &GameState| -> Vec<(u64, (f32, f32))> {
            state.entities().iter().map(|e| (e.uid().raw(), e.position())).collect()
        };
        assert_eq!(positions(&state), positions(&again));
    }

    #[test]
    fn a_frozen_entity_does_not_plan_while_it_is_held() {
        // Its brain would find no action running every tick and ask for work
        // every tick; a search it can never walk is a search for nothing.
        let mut state = world();
        let uid = state.spawn(EntityType::Human, Point::new(4, 4));
        run(&mut state, 30);

        let mut input = Input::new();
        input.freeze(uid, true);
        process_game_state(&mut state, 1.0 / 60.0, &input);

        walker::ROUTES_ASKED.with(|asked| asked.set(0));
        run(&mut state, 600);
        assert_eq!(walker::ROUTES_ASKED.with(|asked| asked.get()), 0);
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
            census,
            facilities,
            intents,
            moves,
            ..
        } = &mut state;
        move_step(&mut entities.freeze(), map, occupancy, census, &facilities.features, &facilities.doors, &mut facilities.panics, &mut facilities.dirt, intents, moves);

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
    fn an_enterable_cell_admits_one_entity_and_blocks_a_second_the_same_way_a_contested_cell_does() {
        // A toilet's cell is impassable in the terrain, exactly like a wall
        // — the one thing different about it is that a `Move` straight onto
        // it is not refused out of hand, only by whoever already holds it.
        // Two entities aimed at it on the same tick settle it exactly the way
        // two entities aimed at the same *empty* cell do.
        let mut map = Map::new(Size::new(6, 2), FLOOR);
        let toilet = Point::new(3, 0);
        crate::sim::testing::prop_at(&mut map, "toilet", toilet);
        // Plumbed in from the row above, or it is not a toilet at all.
        crate::map::utilities::serve_everything(&mut map, Point::new(0, 1), Point::new(3, 1));
        assert!(!map.is_passable(toilet), "a toilet blocks the terrain layer");

        let mut state = GameState::new(map, 7);
        let first = state.spawn(EntityType::Human, Point::new(2, 0));
        let second = state.spawn(EntityType::Human, Point::new(4, 0));

        for uid in [first, second] {
            let slot = state.entities.slot_of(uid).expect("just spawned") as usize;
            state.intents[slot] = Intent::Move { to: (3.5, 0.5) };
        }

        let GameState {
            map,
            entities,
            occupancy,
            census,
            facilities,
            intents,
            moves,
            ..
        } = &mut state;
        move_step(&mut entities.freeze(), map, occupancy, census, &facilities.features, &facilities.doors, &mut facilities.panics, &mut facilities.dirt, intents, moves);

        assert_eq!(state.last_move(first), Some(MoveOutcome::Moved));
        assert_eq!(state.last_move(second), Some(MoveOutcome::Blocked { by: Some(first) }));
        assert_eq!(state.occupancy().occupant(toilet), Some(first));
        assert_eq!(state.entities().get(second).unwrap().center_position(), Point::new(4, 0));
    }

    #[test]
    fn entering_a_bed_cannot_override_a_wall_or_stacked_furniture() {
        let bed = Point::new(3, 0);
        for (terrain, stacked) in [(WALL, false), (FLOOR, true)] {
            let mut map = Map::new(Size::new(6, 2), FLOOR);
            map.set_terrain(bed, terrain);
            crate::sim::testing::prop_at(&mut map, "bed 1", bed);
            if stacked {
                crate::sim::testing::prop_at(&mut map, "crate", bed);
            }
            let mut state = GameState::new(map, 7);
            let uid = state.spawn(EntityType::Human, Point::new(2, 0));
            let slot = state.entities.slot_of(uid).unwrap() as usize;
            state.intents[slot] = Intent::Move { to: (3.5, 0.5) };
            let GameState { map, entities, occupancy, census, facilities, intents, moves, .. } = &mut state;
            move_step(&mut entities.freeze(), map, occupancy, census, &facilities.features, &facilities.doors, &mut facilities.panics, &mut facilities.dirt, intents, moves);
            assert_eq!(state.last_move(uid), Some(MoveOutcome::Blocked { by: None }));
            assert_eq!(state.entities().get(uid).unwrap().center_position(), Point::new(2, 0));
            assert_eq!(state.occupancy().occupant(bed), None);
        }
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

    /// A map with one fridge in it and nothing else.
    fn with_a_fridge(fridge: Point) -> GameState {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        map.add_object(
            crate::map::ObjectLayer::Props,
            crate::map::Object {
                at: Point::new(
                    fridge.x * crate::map::PIXELS_PER_CELL + 24,
                    fridge.y * crate::map::PIXELS_PER_CELL + 24,
                ),
                kind: crate::map::ObjectKind::new("fridge"),
            },
        );
        GameState::new(map, 1)
    }

    #[test]
    fn a_world_step_applies_effects_in_slot_order() {
        // Two entities' effects aimed at the same fridge on the same tick:
        // the later slot's request is the one left standing, because
        // `world_step` walks the slots ascending and applies each in turn.
        let fridge = Point::new(5, 5);
        let mut state = with_a_fridge(fridge);
        let first = state.spawn(EntityType::Human, Point::new(1, 1));
        let second = state.spawn(EntityType::Human, Point::new(1, 2));
        let first_slot = state.entities.slot_of(first).unwrap() as usize;
        let second_slot = state.entities.slot_of(second).unwrap() as usize;
        assert!(first_slot < second_slot, "test assumes arrival order is slot order here");

        state.effects[first_slot] = Effect::Fridge { at: fridge, open: true };
        state.effects[second_slot] = Effect::Fridge { at: fridge, open: false };
        let clock = state.clock();
        world_step(
            &mut state.entities.freeze(),
            &mut state.map,
            &state.log,
            &mut state.facilities,
            &mut state.occupancy,
            &mut state.effects,
            0.0,
            clock,
        );

        assert!(!state.fridges().is_open(fridge), "the later slot's request should have won");
        // The slots are drained either way, so nothing is replayed next tick.
        assert!(state.effects.iter().all(|e| *e == Effect::None));
    }

    #[test]
    fn an_open_fridge_in_a_game_state_warms_tick_over_tick() {
        let fridge = Point::new(5, 5);
        let mut state = with_a_fridge(fridge);
        assert!(state.facilities.fridges.set_open(fridge, true));

        let mut last = state.fridges().temperature(fridge);
        for _ in 0..300 {
            run(&mut state, 1);
            let now = state.fridges().temperature(fridge);
            assert!(now >= last, "should not have cooled while open: {now} after {last}");
            last = now;
        }
        assert!(last > crate::sim::fridge::COLDEST, "never warmed up at all: {last}");
    }

    #[test]
    fn everybody_in_a_district_moves_into_a_bed_of_their_own_house() {
        for seed in [0, 7, 42] {
            let district = crate::map::district::generate(seed);
            let mut state = GameState::new(district.map.clone(), seed);
            assert_eq!(state.spawn_from_spawners(), district.residents(), "seed {seed}");
            let dogs = state.entities().iter().filter(|entity| entity.kind() == Some(EntityType::Dog)).count();
            assert_eq!(state.len() - dogs, district.residents(), "seed {seed}: the residents, and a dog for some of them");
            assert_eq!(state.homes().len(), district.residents(), "everybody has a bed");
            // The bank's computers are the district's desks, and nobody's
            // house has one: every desk is given out, every one in the bank;
            // then the warehouse's four bins, one collector each; then the
            // gates' posts, three watches to a post.
            let desks = state.features().count(FeatureKind::Workstation);
            assert!(desks >= 12, "seed {seed}: {desks} desks");
            let bins = crate::map::district::warehouse::WAREHOUSE_BINS;
            let watches = crate::map::district::GATES * job::WATCHES.len();
            assert_eq!(watches, 24);
            let undertakers = crate::map::district::crematorium::CREMATORIA * job::WATCHES.len();
            let (smelters, couriers) = (crate::map::district::steel_factory::FURNACES, crate::map::district::steel_factory::HOPPERS);
            let farmers = crate::map::district::farm::GRAIN_BINS;
            let operators = crate::map::district::power_plant::CONTROL_PANELS * job::WATCHES.len();
            assert_eq!(operators, 3);
            let kings = crate::map::district::palace::THRONES;
            assert_eq!(kings, 1, "one king");
            let electricians = state.features().count(feature::FeatureKind::DistributionBox) * job::WATCHES.len();
            let food_workers = crate::map::district::food_factory::COOKERS;
            let food_couriers = crate::map::district::food_factory::HOPPERS;
            // The playzone's one computer is run by four: two shifts of two.
            let administrators = crate::map::district::playzone::COMPUTERS * job::PLAYZONE_SHIFTS.len();
            assert_eq!(administrators, 4);
            // The accountant's one computer, beside the throne.
            let accountants = 1;
            let nurses = crate::map::district::hospital::STATIONS * job::NURSE_SHIFTS.len();
            assert_eq!(nurses, 4);
            let mut nursing = 0;
            let mut fishing = 0;
            let mut decorating = 0;
            let fishermen = crate::map::district::warehouse::FISHERMEN;
            // The district itself, kept by four: two shifts of two.
            let cleaners = job::CLEANER_SHIFTS.len();
            let mut cleaning: Vec<(u8, u8)> = Vec::new();
            let jobs = 3 + fishermen + desks + bins + watches + undertakers + smelters + couriers + farmers + operators + kings + electricians + food_workers + food_couriers + administrators + accountants + nurses + cleaners;
            assert!(district.residents() >= jobs, "seed {seed}: more jobs than people");
            assert_eq!(state.workplaces().len(), jobs, "seed {seed}");
            let mut cremating = 0;
            let (mut smelting, mut hauling, mut farming, mut reigning, mut accounting) = (0, 0, 0, 0, 0);
            let (mut clerks, mut collectors) = (0, 0);
            let mut posts: std::collections::BTreeMap<Point, Vec<u8>> = std::collections::BTreeMap::new();
            let mut panels: std::collections::BTreeMap<Point, Vec<u8>> = std::collections::BTreeMap::new();
            let mut computers: std::collections::BTreeMap<Point, Vec<(u8, u8)>> = std::collections::BTreeMap::new();
            for entity in state.entities().iter() {
                let Some(job) = entity.job() else { continue };
                match job.kind {
                    JobKind::OfficeWorker => {
                        clerks += 1;
                        assert!(district.bank.contains(job.workplace), "seed {seed}: a desk outside the bank");
                    }
                    JobKind::ScrapCollector => {
                        collectors += 1;
                        assert!(district.the(crate::map::district::warehouse::NAME).contains(job.workplace), "seed {seed}: a bin outside the warehouse");
                    }
                    JobKind::Guard => {
                        assert!(
                            district.gates.iter().any(|gate| gate.post == job.workplace),
                            "seed {seed}: a guard at {:?}, which is no gate's post",
                            job.workplace
                        );
                        posts.entry(job.workplace).or_default().push(job.shift.start());
                    }
                    JobKind::Undertaker => {
                        cremating += 1;
                        assert!(district.the(crate::map::district::crematorium::NAME).contains(job.workplace), "seed {seed}: a crematorium outside the crematorium");
                    }
                    JobKind::Steelworker | JobKind::Courier => {
                        if job.kind == JobKind::Steelworker {
                            smelting += 1;
                        } else {
                            hauling += 1;
                        }
                        assert!(district.the(crate::map::district::steel_factory::NAME).contains(job.workplace), "seed {seed}: a {} outside the factory", job.kind.name());
                    }
                    JobKind::Farmer => {
                        farming += 1;
                        assert!(district.the(crate::map::district::farm::NAME).contains(job.workplace), "seed {seed}: a grain bin outside the farm");
                    }
                    JobKind::PowerPlantWorker => {
                        assert!(
                            district.the(crate::map::district::power_plant::NAME).contains(job.workplace),
                            "seed {seed}: a control panel outside the power plant"
                        );
                        panels.entry(job.workplace).or_default().push(job.shift.start());
                    }
                    JobKind::FoodFactoryWorker | JobKind::FoodCourier => {
                        assert!(district.the(crate::map::district::food_factory::NAME).contains(job.workplace), "seed {seed}: food job outside its factory");
                    }
                    JobKind::Electrician => {
                        assert!(state.has_box(job.workplace));
                    }
                    JobKind::PlayzoneAdministrator => {
                        assert!(
                            district.the(crate::map::district::playzone::NAME).contains(job.workplace),
                            "seed {seed}: a playzone computer outside the playzone"
                        );
                        computers.entry(job.workplace).or_default().push((job.shift.start(), job.shift.mask()));
                    }
                    JobKind::King => {
                        reigning += 1;
                        assert!(district.palace.contains(job.workplace), "seed {seed}: a throne outside the palace");
                    }
                    JobKind::CityDecorator => {
                        decorating += 1;
                        assert!(district.parks.iter().any(|park| park.contains(job.workplace)));
                    }
                    JobKind::Fisherman => {
                        fishing += 1;
                        assert!(district.the(crate::map::district::warehouse::NAME).contains(job.workplace));
                    }
                    JobKind::Nurse => {
                        nursing += 1;
                        assert!(district.the(crate::map::district::hospital::NAME).contains(job.workplace));
                    }
                    JobKind::Accountant => {
                        accounting += 1;
                        assert!(district.palace.contains(job.workplace), "seed {seed}: the accountant outside the palace");
                    }
                    JobKind::Cleaner => {
                        assert!(state.features().has(feature::FeatureKind::District, job.workplace));
                        cleaning.push((job.shift.start(), job.shift.mask()));
                    }
                }
                assert!(state.properties().of(job.workplace).is_none(), "and in nobody's house");
            }
            assert_eq!(nursing, nurses);
            assert_eq!(fishing, fishermen);
            assert_eq!(decorating, 3);
            assert_eq!((clerks, collectors, cremating), (desks, bins, undertakers), "seed {seed}");
            assert_eq!((smelting, hauling), (smelters, couriers), "seed {seed}: the steel factory's");
            assert_eq!(farming, farmers, "seed {seed}: the farm's");
            assert_eq!(reigning, kings, "seed {seed}: the palace's");
            assert_eq!(accounting, accountants, "seed {seed}: the palace's accountant");
            // Two cleaners every day, one after the other.
            let mut shifts = job::CLEANER_SHIFTS.map(|shift| (shift.start(), shift.mask()));
            assert_eq!(shifts, [(7, job::FIRST_GROUP), (7, job::SECOND_GROUP), (15, job::FIRST_GROUP), (15, job::SECOND_GROUP)]);
            shifts.sort_unstable();
            cleaning.sort_unstable();
            assert_eq!(cleaning, shifts, "seed {seed}");
            let last = state.entities().iter().filter_map(|entity| entity.job()).map(|job| job.kind).last();
            assert_eq!(last, Some(JobKind::Cleaner), "seed {seed}: cleaners hired last");
            // Every gate's post kept by three guards, one a watch, round the
            // clock.
            assert_eq!(posts.len(), crate::map::district::GATES, "seed {seed}");
            for (post, mut starts) in posts {
                starts.sort_unstable();
                assert_eq!(starts, [0, 8, 16], "seed {seed}: the watches at {post:?}");
            }
            // The power plant's control panel kept the same way, by its three
            // workers.
            assert_eq!(panels.len(), crate::map::district::power_plant::CONTROL_PANELS, "seed {seed}");
            for (panel, mut starts) in panels {
                starts.sort_unstable();
                assert_eq!(starts, [0, 8, 16], "seed {seed}: the watches at the control panel at {panel:?}");
            }
            // The playzone's computer run by four: a day and an evening shift
            // for each of the two groups, so every day of the week has a
            // day shift and an evening shift and two administrators on them.
            assert_eq!(computers.len(), crate::map::district::playzone::COMPUTERS, "seed {seed}");
            for (computer, mut shifts) in computers {
                shifts.sort_unstable();
                let (first, second) = (job::FIRST_GROUP, job::SECOND_GROUP);
                let mut expected = [(10, first), (10, second), (16, first), (16, second)];
                expected.sort_unstable();
                assert_eq!(shifts, expected, "seed {seed}: the shifts at {computer:?}");
            }
            // In the order they arrived: every desk taken before the first
            // collector was hired, every bin before the first guard.
            let kinds: Vec<JobKind> =
                state.entities().iter().filter_map(|entity| entity.job()).map(|job| job.kind).collect();
            let order = |kind: JobKind| JobKind::ALL.iter().position(|&k| k == kind).unwrap();
            assert!(kinds.windows(2).all(|pair| order(pair[0]) <= order(pair[1])), "seed {seed}: {kinds:?}");

            for house in &district.houses {
                for &bed in &house.beds {
                    let owner = state.homes().owner(bed).expect("an empty bed in somebody's house");
                    let lives_at = state.entities().get(owner).unwrap().center_position();
                    assert!(house.walls.contains(lives_at), "seed {seed}: {owner} sleeps away from home");
                }
            }
        }
    }

    /// A district's gates on a Monday, from ten in the morning to six in the
    /// evening, the guards' needs switched off as the collectors' are below
    /// (with them on, the district's crowd limit takes the guards into the
    /// bank's jam with everybody else): the day watch at its posts, the
    /// evening watch walking up across the district before four and taking
    /// over. Looked at every quarter of an hour, nearly every post is kept
    /// at nearly every look.
    #[test]
    fn a_district_keeps_its_gates_through_the_change_of_watch() {
        let district = crate::map::district::generate(7);
        let mut state = GameState::new(district.map.clone(), 7);
        state.spawn_from_spawners();
        let mut input = Input::new();
        for guard in state.entities().iter().filter(|e| e.job().is_some_and(|job| job.kind == JobKind::Guard)) {
            for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
                input.set_process(guard.uid(), process, false);
            }
        }
        assert_eq!(input.commands.len(), 4 * 24);
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let posts: Vec<Point> = district.gates.iter().map(|gate| gate.post).collect();
        // 1800 ticks a world hour: to ten, then a look every quarter of an
        // hour to six.
        run(&mut state, 2 * 1800);
        let (mut looks, mut kept) = (0, 0);
        for _ in 0..32 {
            run(&mut state, 450);
            state.log.drain();
            looks += posts.len();
            kept += posts
                .iter()
                .filter(|&&post| state.entities().iter().any(|entity| entity.interacting_with() == Some(post)))
                .count();
        }
        assert!(kept * 10 >= looks * 9, "{kept} of {looks} post-looks kept, by {}", state.clock());
    }

    /// A district's playzone on a Monday (the first group's day), its
    /// administrators' needs switched off as the guards' are above: its
    /// arcade cabinets are dark in the morning, all of them work from ten
    /// o'clock — through the change from the day shift to the evening shift
    /// at four — and are dark again from ten at night. Looked at every
    /// quarter of an hour.
    #[test]
    fn a_district_s_playzone_is_run_from_ten_to_ten_and_dark_outside_those_hours() {
        use crate::map::district::playzone::CABINETS;
        let district = crate::map::district::generate(7);
        let mut state = GameState::new(district.map.clone(), 7);
        state.spawn_from_spawners();
        let mut input = Input::new();
        let administrators: Vec<Uid> = state
            .entities()
            .iter()
            .filter(|e| e.job().is_some_and(|job| job.kind == JobKind::PlayzoneAdministrator))
            .map(|e| e.uid())
            .collect();
        assert_eq!(administrators.len(), 4);
        for &administrator in &administrators {
            for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
                input.set_process(administrator, process, false);
            }
        }
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let working = |state: &GameState| state.facilities.playzones.working();
        assert_eq!(working(&state), 0, "nobody is there at eight");

        // 1800 ticks a world hour, from eight o'clock: a look every quarter of
        // an hour to eleven at night.
        let (mut dark_by_day, mut kept, mut looks, mut dark_by_night) = (0, 0, 0, 0);
        let mut when = Vec::new();
        for quarter in 0..60 {
            run(&mut state, 450);
            state.log.drain();
            let hours = 8.0 + (quarter + 1) as f32 / 4.0;
            if hours < 9.99 {
                dark_by_day += working(&state);
            } else if (10.5..21.75).contains(&hours) {
                looks += 1;
                kept += usize::from(working(&state) == CABINETS);
                when.push((hours, working(&state)));
            } else if hours > 22.01 {
                dark_by_night += working(&state);
            }
        }
        assert_eq!(dark_by_day, 0, "cabinets working before ten");
        assert_eq!(dark_by_night, 0, "cabinets working after ten");
        assert!(kept * 10 >= looks * 9, "{kept} of {looks} looks had the playzone run: {when:?}");
    }

    /// A district's collectors, on a Monday: out to the heaps round it from
    /// half past eight, and before the shift is over the first packfuls are
    /// in the warehouse's bins. Their needs are switched off: seed 7's
    /// collectors live across the district from the warehouse, and a thirsty
    /// one out at the yards walks home through a district jammed round the
    /// bank (the crowd limit `map-generator` describes) — this is about the
    /// job.
    #[test]
    fn a_district_on_a_monday_stores_scrap_in_its_warehouse() {
        let district = crate::map::district::generate(7);
        let mut state = GameState::new(district.map.clone(), 7);
        state.spawn_from_spawners();
        let mut input = Input::new();
        for collector in state.entities().iter().filter(|e| e.job().is_some_and(|job| job.kind == JobKind::ScrapCollector)) {
            for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
                input.set_process(collector.uid(), process, false);
            }
        }
        assert_eq!(input.commands.len(), 4 * crate::map::district::warehouse::WAREHOUSE_BINS);
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let mut lines = Vec::new();
        // 1800 ticks a world hour at `run`'s 60Hz: until five o'clock at the
        // latest.
        for _ in 0..9 * 1800 {
            run(&mut state, 1);
            lines.extend(state.log.drain().into_iter().filter(|line| line.contains("scrap")));
            if state.bins().total_in(feature::FeatureKind::ScrapBin) > 0 {
                break;
            }
        }
        assert!(state.bins().total_in(feature::FeatureKind::ScrapBin) > 0, "nothing stored by {}; {:?}", state.clock(), &lines[lines.len().saturating_sub(20)..]);
        assert!(lines.iter().any(|line| line.contains("salvaged scrap metal")));
        let stored = lines.iter().find(|line| line.contains("in the bin at")).expect("a line about it");
        assert!(stored.contains("scrap metal in the bin at"), "{stored}");
        for (bin, store, _) in state.bins().iter_stores() {
            let building = match store.store().unwrap().site {
                "factory" => district.the(crate::map::district::steel_factory::NAME),
                "farm" => district.the(crate::map::district::farm::NAME),
                "food factory" => district.the(crate::map::district::food_factory::NAME),
                _ => district.the(crate::map::district::warehouse::NAME),
            };
            assert!(building.contains(bin), "{store:?} at {bin:?}");
        }
    }

    /// Seed 7 on a Monday, its farmers' needs off as the collectors' are
    /// above, and their tiredness — a resident arrives as tired as it
    /// happens to be, and a nap would take one home: out through the gates
    /// to the good soil by the farm, and every one of them with a field of
    /// its own sown by five.
    #[test]
    fn a_district_on_a_monday_sows_its_fields() {
        let district = crate::map::district::generate(7);
        let mut state = GameState::new(district.map.clone(), 7);
        state.spawn_from_spawners();
        let mut input = Input::new();
        let farmers: Vec<Uid> = state.entities().iter().filter(|e| e.job().is_some_and(|job| job.kind == JobKind::Farmer)).map(|e| e.uid()).collect();
        assert_eq!(farmers.len(), crate::map::district::farm::GRAIN_BINS);
        for &farmer in &farmers {
            for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun, ProcessId::Energy] {
                input.set_process(farmer, process, false);
            }
        }
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let fields = farmers.len() * fields::PLOTS;
        // Measured: the last crop in at 14:43.
        for _ in 0..9 * 1800 {
            run(&mut state, 1);
            if state.fields().len() == fields {
                break;
            }
        }
        assert_eq!(state.fields().len(), fields, "sown by {}", state.clock());
        let farm = district.the(crate::map::district::farm::NAME);
        for crop in state.fields().iter() {
            assert!(farm.contains(crop.farm), "{crop:?} for a farm that is not the farm");
            assert!(!state.map.contains(crop.cell) && state.map.terrain(crop.cell).is_some_and(|tile| tile.is_fertile()), "{crop:?}");
        }
        for &farmer in &farmers {
            let bin = state.entities().get(farmer).unwrap().job().unwrap().workplace;
            assert_eq!(state.fields().of(bin).count(), fields::PLOTS, "{farmer}'s field");
        }

    }

    /// A district's cleaners, their needs off, mop up the stains put on its
    /// public ground on a Monday morning — the first group's morning cleaner
    /// is on from seven — and leave a home's alone.
    #[test]
    fn a_district_s_morning_cleaner_mops_up_its_public_ground() {
        let district = crate::map::district::generate(7);
        let mut state = GameState::new(district.map.clone(), 7);
        state.spawn_from_spawners();
        let mut input = Input::new();
        let cleaners: Vec<Uid> = state
            .entities()
            .iter()
            .filter(|e| e.job().is_some_and(|job| job.kind == JobKind::Cleaner))
            .map(|e| e.uid())
            .collect();
        assert_eq!(cleaners.len(), job::CLEANER_SHIFTS.len());
        for &cleaner in &cleaners {
            for process in ProcessId::ALL {
                input.set_process(cleaner, process, false);
            }
        }
        // A dozen stains on public ground round the middle, and one in a home.
        let middle = state.features().cells_of(feature::FeatureKind::District)[0];
        let public = |cell: Point| state.map.is_passable(cell) && state.properties().of(cell).is_none();
        let stains: Vec<Point> = state
            .map
            .size()
            .points()
            .filter(|&cell| cell.x % 9 == 0 && cell.y % 7 == 0 && cell.manhattan_distance(middle) <= 40 && public(cell))
            .take(12)
            .collect();
        assert_eq!(stains.len(), 12);
        let home = state
            .map
            .size()
            .points()
            .find(|&cell| state.map.is_passable(cell) && state.properties().of(cell).is_some())
            .unwrap();
        for &cell in stains.iter().chain([&home]) {
            input.set_dirt(cell, dirt::Dirt::earth(0.6));
        }
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let mut lines = Vec::new();
        for _ in 0..4 {
            run(&mut state, 1800);
            lines.extend(state.log.drain());
        }
        let cleaned_up = |cell: Point| {
            lines.iter().any(|line| line.contains("cleaned up") && line.ends_with(&format!(" at {}, {}", cell.x, cell.y)))
        };
        // Measured: all twelve by noon, and two more stains that people had
        // tracked in through a gate.
        let missed: Vec<Point> = stains.iter().copied().filter(|&cell| !cleaned_up(cell)).collect();
        assert!(missed.is_empty(), "not cleaned by {}: {missed:?}", state.clock());
        assert!(!cleaned_up(home), "nobody's home is cleaned");
        // From eight to twelve, a look every 27 minutes on the minute: at
        // 08:00, 08:27, ... 11:36. The evening cleaners are not on yet.
        let looks = lines.iter().filter(|line| line.contains("looked over the district")).count();
        assert_eq!(looks, 9);
    }

    #[test]
    fn adopting_a_dog_bonds_it_to_its_owner_both_ways() {
        let mut state = world();
        let owner = state.spawn(EntityType::Human, Point::new(5, 5));
        let dog = state.adopt_dog(owner).expect("a dog");
        assert_eq!(dog.kind(), Some(EntityType::Dog));
        assert_eq!(state.entities().get(owner).unwrap().companion(), Some(dog));
        assert_eq!(state.entities().get(dog).unwrap().companion(), Some(owner));
        let beside = state.entities().get(dog).unwrap().center_position().chebyshev_distance(Point::new(5, 5));
        assert_eq!(beside, 1, "in the cell next to its owner");
        assert_eq!(state.adopt_dog(owner), None, "one dog each");
        assert_eq!(state.adopt_dog(dog), None, "a dog does not keep a dog");
        assert_eq!(state.adopt_dog(Uid::new(EntityType::Human, 12345)), None, "nobody to adopt it");
    }

    #[test]
    fn a_dog_is_not_adopted_where_there_is_no_room_for_it() {
        let mut map = Map::new(Size::new(9, 9), WALL);
        map.set_terrain(Point::new(4, 4), FLOOR);
        let mut state = GameState::new(map, 1);
        let owner = state.spawn(EntityType::Human, Point::new(4, 4));
        assert_eq!(state.adopt_dog(owner), None);
        assert_eq!(state.len(), 1);
        assert_eq!(state.entities().get(owner).unwrap().companion(), None);
    }

    /// One resident in eight has a dog, beside it, with its key; nobody has two.
    #[test]
    fn a_district_gives_some_of_its_residents_a_pet_dog_beside_them() {
        for seed in [0, 7, 42] {
            let district = crate::map::district::generate(seed);
            let mut state = GameState::new(district.map.clone(), seed);
            state.spawn_from_spawners();
            let dogs: Vec<Uid> = state
                .entities()
                .iter()
                .filter(|entity| entity.kind() == Some(EntityType::Dog))
                .map(|entity| entity.uid())
                .collect();
            let residents = district.residents();
            assert!(
                (residents / 16..=residents / 4).contains(&dogs.len()),
                "seed {seed}: {} dogs for {residents} residents",
                dogs.len()
            );
            let mut owners = std::collections::BTreeSet::new();
            for dog in dogs {
                let pet = state.entities().get(dog).unwrap();
                let owner = pet.companion().expect("a pet has an owner");
                let keeper = state.entities().get(owner).unwrap();
                assert_eq!(keeper.kind(), Some(EntityType::Human), "seed {seed}");
                assert_eq!(keeper.companion(), Some(dog), "seed {seed}: bonded both ways");
                assert!(keeper.body().home().is_some(), "seed {seed}: an owner has a home");
                assert_eq!(pet.body().home(), keeper.body().home(), "seed {seed}: a dog goes where its owner may");
                assert!(pet.center_position().chebyshev_distance(keeper.center_position()) <= 3, "seed {seed}");
                assert!(owners.insert(owner), "seed {seed}: two dogs for one owner");
            }
        }
    }

    /// A 12x12 floor with a fridge at (6, 0) that works: on a powered line
    /// along the top edge, off a transformer at (3, 0).
    fn with_a_powered_fridge() -> (GameState, Point) {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for (x, c) in "-B=B=B-".chars().enumerate() {
            map.set_grid(crate::map::GridLayer::Power, Point::new(x as i32, 0), crate::map::GridLayer::Power.value_of(c).unwrap());
        }
        for (name, x) in [("transformer", 3), ("fridge", 6)] {
            map.add_object(
                ObjectLayer::Props,
                crate::map::Object {
                    at: Point::new(x * crate::map::PIXELS_PER_CELL + 24, 24),
                    kind: crate::map::ObjectKind::new(name),
                },
            );
        }
        let state = GameState::new(map, 1);
        let fridge = Point::new(6, 0);
        assert!(state.features().has(FeatureKind::Food, fridge), "the fridge works");
        (state, fridge)
    }

    /// The whole of it in one small world: a hungry dog is bought a dinner
    /// out of the fridge by its owner and fed it, and hours later, the dinner
    /// having passed through, fouls the street.
    #[test]
    fn a_hungry_dog_is_fed_by_its_owner_and_hours_later_fouls_the_street() {
        let (mut state, _fridge) = with_a_powered_fridge();
        let owner = state.spawn(EntityType::Human, Point::new(3, 6));
        let mut input = Input::new();
        for process in ProcessId::ALL {
            input.set_process(owner, process, false);
        }
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let dog = state.adopt_dog(owner).unwrap();
        state.entity_mut(dog).unwrap().starve_to(90.0);
        let cash = |state: &GameState| state.facilities.economy.account(owner).unwrap().cash;
        let before = cash(&state);

        let mut lines = Vec::new();
        let said = |lines: &[String], what: &str| lines.iter().filter(|line| line.contains(what)).count();
        for _ in 0..80 {
            run(&mut state, 100);
            lines.extend(state.log.drain());
            if said(&lines, "fouled the street") > 0 {
                break;
            }
        }
        let fed = said(&lines, &format!("{owner} fed {dog}"));
        let paid = said(&lines, "paid $1.00 for dog food");
        assert!(fed >= 1, "never fed: {lines:?}");
        assert_eq!(paid, fed, "a dollar a dinner");
        assert_eq!(before - cash(&state), economy::Money::dollars(paid as i64));
        assert!(said(&lines, "fouled the street") >= 1, "never fouled the street by {}", state.clock());
        let stained = state.map.size().points().any(|cell| state.dirt().at(cell).overall() > 0.15);
        assert!(stained, "the street is clean");
    }

    #[test]
    fn in_a_district_the_bored_watch_the_tvs_in_their_own_houses() {
        let district = crate::map::district::generate(7);
        let tvs: Vec<String> = district
            .houses
            .iter()
            .filter_map(|house| house.tv)
            .map(|tv| format!("watched the TV at {}, {}", tv.x, tv.y))
            .collect();
        assert!(!tvs.is_empty());
        let mut state = GameState::new(district.map.clone(), 7);
        state.spawn_from_spawners();
        let mut watched = Vec::new();
        // 1800 ticks a world hour: a day at most.
        for _ in 0..24 * 1800 {
            run(&mut state, 1);
            watched.extend(state.log.drain().into_iter().filter(|line| line.contains("watched the TV")));
            if watched.len() >= 3 {
                break;
            }
        }
        assert!(watched.len() >= 3, "{} TVs watched by {}", watched.len(), state.clock());
        for line in &watched {
            assert!(tvs.iter().any(|tv| line.ends_with(tv.as_str())), "{line}: not a house's TV");
        }
    }

    /// The district's own steel works: its collectors bring scrap in to the
    /// warehouse, its couriers take it to the steel factory, its steelworkers
    /// smelt it and the couriers bring the steel back to the warehouse's
    /// rack. The three trades' needs off, as the scrap test has the
    /// collectors' (the crowd's limits are not this test's).
    #[test]
    fn a_district_s_steel_works_bring_steel_into_its_warehouse() {
        let district = crate::map::district::generate(7);
        let mut state = GameState::new(district.map.clone(), 7);
        state.spawn_from_spawners();
        let mut input = Input::new();
        let trades = [JobKind::ScrapCollector, JobKind::Courier, JobKind::Steelworker];
        for worker in state.entities().iter().filter(|e| e.job().is_some_and(|job| trades.contains(&job.kind))) {
            for process in ProcessId::ALL {
                input.set_process(worker.uid(), process, false);
            }
        }
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let rack = state
            .bins()
            .iter_stores()
            .find(|&(_, store, _)| store == FeatureKind::SteelRack)
            .map(|(cell, _, _)| cell)
            .expect("the warehouse has a rack");
        assert!(district.the(crate::map::district::warehouse::NAME).contains(rack));
        // Monday and Tuesday, office hours and the night between.
        let mut ticks = 0;
        while state.bins().stored(rack) == 0 {
            run(&mut state, 60);
            ticks += 60;
            assert!(ticks < 34 * 1800, "no steel in the warehouse by {}; {:?}", state.clock(), state.statistics());
        }
        let stats = state.statistics();
        assert!(stats.stock("warehouse", ItemKind::Steel) > 0);
    }

    #[test]
    fn wheat_from_the_farm_becomes_meals_in_the_warehouse_without_feeding_anybody() {
        let (mut state, worker, courier) = crate::sim::testing::food_works(12, 3);
        run(&mut state, 8 * 1800);
        assert!(state.bins().total_in(feature::FeatureKind::MealRack) >= 2, "{:?}", state.statistics());
        let stock: u32 = state.bins().iter().map(|(_, n)| n).sum();
        let carried: u32 = [worker, courier].into_iter().map(|uid| {
            let pack = state.entities().get(uid).unwrap().inventory().unwrap();
            u32::from(pack.count(ItemKind::Wheat)) + u32::from(pack.count(ItemKind::Meal))
        }).sum();
        assert_eq!(stock + carried, 12, "one wheat per meal, with nothing lost or duplicated");
        assert_eq!(ItemKind::Meal.nutrition(), 0.0, "stored goods are outside the food chain");
        assert_eq!(state.features().count(feature::FeatureKind::Food), 0);
        assert_eq!(state.statistics().stock("warehouse", ItemKind::Meal), state.bins().total_in(feature::FeatureKind::MealRack));
    }

    #[test]
    fn food_production_is_deterministic_in_the_parallel_rounds() {
        let build = || {
            let (mut state, _, _) = crate::sim::testing::food_works(24, 5);
            for i in 0..PARALLEL_AT {
                state.spawn(EntityType::Dog, Point::new(i as i32 % 24, 13 + i as i32 / 24));
            }
            state
        };
        let (mut a, mut b) = (build(), build());
        assert!(a.len() > PARALLEL_AT);
        for _ in 0..6 * 1800 {
            process_game_state(&mut a, 1.0 / 60.0, &Input::new());
            process_game_state(&mut b, 1.0 / 60.0, &Input::new());
        }
        assert!(a.bins().total_in(feature::FeatureKind::MealRack) > 0);
        assert_eq!(a.bins(), b.bins());
        for (left, right) in a.entities().iter().zip(b.entities().iter()) {
            assert_eq!(left.save(), right.save());
        }
    }

    #[test]
    fn a_new_district_starts_with_thirty_warehouse_meals_even_after_saving_the_map() {
        for seed in [0, 7, 42] {
            let district = crate::map::district::generate(seed);
            let map = Map::from_json(&district.map.to_json().unwrap()).unwrap();
            let mut state = GameState::new(map, seed);
            assert_eq!(state.bins().total_in(feature::FeatureKind::MealRack), 30);
            state.spawn_from_spawners();
            assert_eq!(state.bins().total_in(feature::FeatureKind::MealRack), 30);
            assert_eq!(state.bins().total_in(feature::FeatureKind::MealPallet), 0);
        }
    }

    #[test]
    fn switching_a_box_off_takes_its_fridges_away_and_switching_it_on_gives_them_back() {
        let district = crate::map::district::generate(7);
        let house = &district.houses[0];
        let mut state = GameState::new(district.map.clone(), 42);
        let fridge = state
            .map
            .objects(ObjectLayer::Props)
            .iter()
            .find(|prop| prop.kind.as_str() == "fridge" && house.walls.contains(prop.cell()))
            .map(|prop| prop.cell())
            .expect("every house has a fridge");
        let at = house
            .walls
            .cells()
            .find(|&cell| state.has_box(cell))
            .expect("every house has a box");
        let fed = state.features().count(FeatureKind::Food);
        assert!(state.features().has(FeatureKind::Food, fridge));
        assert!(state.box_is_on(at));

        let mut input = Input::new();
        input.switch_box(at, false);
        spawn_pass(&mut state, &input);
        assert!(!state.box_is_on(at));
        assert!(!state.features().has(FeatureKind::Food, fridge), "a fridge on a dead box");
        assert_eq!(state.features().count(FeatureKind::Food), fed - 1, "and only that one");
        assert!(!state.supply().serves("fridge", fridge));
        assert!(!state.fridges().get(fridge).unwrap().is_powered());
        assert_eq!(state.supply_generation(), 1);

        let mut input = Input::new();
        input.switch_box(at, true);
        spawn_pass(&mut state, &input);
        assert!(state.features().has(FeatureKind::Food, fridge));
        assert!(state.fridges().get(fridge).unwrap().is_powered());

        assert!(!state.switch_box(Point::new(0, 0), false), "there is no box there");
    }

    /// A street lamp on a box with a controller on it, and a fridge on a
    /// box with none, both off one line: the lamp has power from dusk to
    /// dawn, the fridge all day and all night.
    #[test]
    fn a_street_light_controller_powers_its_lamps_from_dusk_to_dawn() {
        use crate::map::grid::power;
        use crate::map::utilities::CONTROLLER;
        let mut map = Map::new(Size::new(7, 1), FLOOR);
        for (x, c) in "-B=B=B-".chars().enumerate() {
            map.set_grid(crate::map::GridLayer::Power, Point::new(x as i32, 0), crate::map::GridLayer::Power.value_of(c).unwrap());
        }
        let put = |map: &mut Map, name: &str, x: i32| {
            map.add_object(
                ObjectLayer::Props,
                crate::map::Object { at: Point::new(x * 48 + 24, 24), kind: crate::map::ObjectKind::new(name) },
            )
        };
        // The transformer stands on the middle box; the lamp's box at the
        // left end has the controller, the fridge's at the right does not.
        put(&mut map, "transformer", 3);
        put(&mut map, "street lamp", 0);
        put(&mut map, CONTROLLER, 1);
        put(&mut map, "fridge", 6);
        assert_eq!(map.grid(crate::map::GridLayer::Power, Point::new(1, 0)), power::BOX);
        let (lamp, fridge) = (Point::new(0, 0), Point::new(6, 0));

        let mut state = GameState::new(map, 1);
        assert!(state.is_controlled(Point::new(1, 0)) && !state.is_controlled(Point::new(5, 0)));
        // One tick ending at the hour asked for: the controllers look at the
        // sky in the processing pass, not the spawn pass.
        let at = |state: &mut GameState, hours_after_opening: f64| {
            let dt = 1.0 / 64.0;
            state.elapsed = hours_after_opening * clock::HOUR as f64 / clock::TIME_SCALE as f64 - dt as f64;
            process_pass(state, dt);
            (state.supply().serves("street lamp", lamp), state.supply().serves("fridge", fridge))
        };
        assert!(!state.street_lights_on(), "a world opens at eight in the morning");
        assert_eq!(at(&mut state, 0.0), (false, true), "eight in the morning");
        assert_eq!(state.supply_generation(), 0, "nothing changed, nothing re-flooded");
        assert_eq!(at(&mut state, 11.0), (false, true), "seven in the evening: still light");
        assert_eq!(at(&mut state, 15.0), (true, true), "eleven at night");
        assert!(state.street_lights_on());
        assert_eq!(state.supply_generation(), 1);
        assert_eq!(at(&mut state, 20.0), (true, true), "four in the morning");
        assert_eq!(at(&mut state, 24.0), (false, true), "eight the next morning");
        assert_eq!(state.supply_generation(), 2);

        // Switched off by hand, the controller cannot put it back on.
        assert!(state.switch_box(Point::new(1, 0), false));
        assert_eq!(at(&mut state, 39.0), (false, true), "eleven the next night, switched off");
        assert!(state.switch_box(Point::new(1, 0), true));
        assert!(state.supply().serves("street lamp", lamp));
        assert_eq!(at(&mut state, 48.0), (false, true), "and off again at dawn");
    }

    /// The lights change on the first tick whose clock the sun calls dark —
    /// in the processing pass, so not on whichever frame happens to run the
    /// spawn pass next: a run is the same at any game speed.
    #[test]
    fn the_street_lights_change_on_the_tick_the_sun_says_and_no_other() {
        use crate::map::utilities::CONTROLLER;
        let mut map = Map::new(Size::new(3, 1), FLOOR);
        for (x, c) in "B--".chars().enumerate() {
            map.set_grid(crate::map::GridLayer::Power, Point::new(x as i32, 0), crate::map::GridLayer::Power.value_of(c).unwrap());
        }
        for (name, x) in [("transformer", 0), (CONTROLLER, 0), ("street lamp", 2)] {
            map.add_object(
                ObjectLayer::Props,
                crate::map::Object { at: Point::new(x * 48 + 24, 24), kind: crate::map::ObjectKind::new(name) },
            );
        }
        let mut state = GameState::new(map, 1);
        let dt = 1.0 / 64.0;
        // From half past six in the evening, a tick at a time and no spawn
        // pass at all.
        state.elapsed = 10.5 * clock::HOUR as f64 / clock::TIME_SCALE as f64;
        for _ in 0..64 * 60 {
            process_pass(&mut state, dt);
            assert_eq!(
                state.street_lights_on(),
                state.clock().sun().is_dark(),
                "at {}: the lights lag the sun",
                state.clock().time()
            );
        }
        assert!(state.street_lights_on(), "it is dark by half past eight");
        assert_eq!(state.supply_generation(), 1, "one switch, one swap");
    }

    /// A walled, roofed room with a ceiling lamp and a fridge on one wired
    /// circuit, and a light switch by the far wall: off, the lamp has no
    /// power and the fridge keeps it; on again, the lamp has it back.
    #[test]
    fn a_light_switch_takes_its_room_s_lamps_off_and_leaves_its_fridge_on() {
        use crate::map::utilities::{serve_everything, LIGHT_SWITCH};
        use crate::map::WALL;
        let mut map = Map::new(Size::new(9, 7), FLOOR);
        for cell in map.size().points().collect::<Vec<_>>() {
            map.set_ceiling(cell, true);
            if cell.x == 0 || cell.y == 0 || cell.x == 8 || cell.y == 6 {
                map.set_terrain(cell, WALL);
            }
        }
        let put = |map: &mut Map, layer: ObjectLayer, name: &str, cell: Point| {
            map.add_object(layer, crate::map::Object { at: Point::new(cell.x * 48 + 24, cell.y * 48 + 24), kind: crate::map::ObjectKind::new(name) })
        };
        let (lamp, fridge, switch) = (Point::new(4, 3), Point::new(6, 1), Point::new(2, 5));
        put(&mut map, ObjectLayer::Lamps, "ceiling lamp", lamp);
        put(&mut map, ObjectLayer::Props, "fridge", fridge);
        put(&mut map, ObjectLayer::Props, LIGHT_SWITCH, switch);
        serve_everything(&mut map, Point::new(0, 0), Point::new(8, 0));

        let mut state = GameState::new(map, 1);
        assert!(state.has_light_switch(switch) && state.light_is_on(switch), "a world begins with its lights on");
        assert!(state.supply().serves("ceiling lamp", lamp));
        let food = state.features().count(FeatureKind::Food);

        let mut input = Input::new();
        input.switch_light(switch, false);
        spawn_pass(&mut state, &input);
        assert!(!state.light_is_on(switch));
        assert!(!state.supply().serves("ceiling lamp", lamp), "switched off");
        assert!(state.supply().serves("fridge", fridge), "the fridge is not on the switch");
        assert!(state.fridges().get(fridge).unwrap().is_powered());
        assert_eq!(state.features().count(FeatureKind::Food), food);
        assert_eq!(state.supply_generation(), 1);
        assert!(state.log.drain().iter().any(|line| line.contains("the light switch at 2, 5 was switched off")));

        // Already off: nothing to do, nothing re-flooded.
        assert!(state.switch_light(switch, false));
        assert_eq!(state.supply_generation(), 1);

        // A box switched off and on again does not switch the light back on.
        let a_box = state.map.size().points().find(|&cell| state.has_box(cell)).unwrap();
        assert!(state.switch_box(a_box, false) && state.switch_box(a_box, true));
        assert!(!state.supply().serves("ceiling lamp", lamp));

        assert!(state.switch_light(switch, true));
        assert!(state.supply().serves("ceiling lamp", lamp));
        assert!(!state.switch_light(Point::new(4, 4), false), "there is no switch there");
    }

    #[test]
    fn a_spawner_of_a_kind_nobody_knows_spawns_nobody() {
        let mut map = Map::new(Size::new(4, 4), FLOOR);
        let at = |kind: &str| crate::map::Object {
            at: Point::new(24, 24),
            kind: crate::map::ObjectKind::new(kind),
        };
        map.add_object(ObjectLayer::Spawners, at("unicorn"));
        map.add_object(ObjectLayer::Spawners, at("dog"));
        let mut state = GameState::new(map, 1);
        assert_eq!(state.spawn_from_spawners(), 1);
        assert_eq!(state.len(), 1);
    }

    /// A wall across the map with one door in it, a fridge on the far side
    /// and a hungry human on the near side.
    fn a_door_between_a_human_and_a_meal() -> (testing::World, kinds::Human, Point) {
        let mut map = Map::new(Size::new(12, 7), FLOOR);
        for y in 0..7 {
            map.set_terrain(Point::new(6, y), WALL);
        }
        let door = Point::new(6, 3);
        map.set_terrain(door, FLOOR);
        testing::prop_at(&mut map, door::DOOR, door);
        testing::prop_at(&mut map, "fridge", Point::new(10, 3));
        let mut human = testing::needy_human(Point::new(2, 3), 90.0, 0.0, 0.0);
        human.biology_mut().unwrap().set_running(ProcessId::Thirst, false);
        human.biology_mut().unwrap().set_running(ProcessId::Bladder, false);
        (testing::World::new(map), human, door)
    }

    #[test]
    fn a_unit_stands_still_at_a_shut_door_while_it_opens_and_then_walks_through() {
        let (mut world, mut human, door) = a_door_between_a_human_and_a_meal();
        assert!(world.doors.is_shut(door));
        let mut waited = 0;
        let mut was = human.position();
        for _ in 0..4000 {
            world.step(&mut human);
            let now = human.position();
            assert!(
                human.center_position() != door || !world.doors.is_shut(door),
                "stood in a doorway whose door was not open"
            );
            let opening = world.doors.openness(door);
            if opening > 0.0 && opening < 1.0 && now == was && human.center_position() == Point::new(5, 3) {
                waited += 1;
            }
            was = now;
            if world.meals() > 0 {
                break;
            }
        }
        assert_eq!(world.meals(), 1, "it got through to the fridge");
        // Opening takes `door::OPENING`, every tick of which it stood still.
        let ticks = (door::OPENING / world.dt) as usize;
        assert!(waited + 2 >= ticks, "waited {waited} ticks of an opening {ticks} long");
    }

    #[test]
    fn a_door_nobody_is_going_through_shuts_behind_them() {
        let (mut world, mut human, door) = a_door_between_a_human_and_a_meal();
        for _ in 0..4000 {
            world.step(&mut human);
            if world.meals() > 0 {
                break;
            }
        }
        assert_eq!(world.meals(), 1);
        assert_eq!(world.doors.openness(door), 0.0, "shut again by the time the meal was over");
    }

    /// The walled house from `property`'s tests with a fridge inside, and a
    /// hungry human outside — given the key to it, or not.
    fn a_house_with_a_fridge(owner: bool) -> (testing::World, kinds::Human) {
        let mut map = property::tests::house();
        testing::prop_at(&mut map, "fridge", Point::new(5, 5));
        let world = testing::World::new(map);
        let mut human = testing::needy_human(Point::new(5, 0), 90.0, 0.0, 0.0);
        human.biology_mut().unwrap().set_running(ProcessId::Thirst, false);
        human.biology_mut().unwrap().set_running(ProcessId::Bladder, false);
        if owner {
            let home = world.properties.of(Point::new(5, 5));
            assert!(home.is_some());
            assert!(human.set_home(Point::new(3, 6), home));
        }
        (world, human)
    }

    #[test]
    fn the_owner_of_a_house_lets_themselves_in_to_eat() {
        let (mut world, mut human) = a_house_with_a_fridge(true);
        for _ in 0..4000 {
            world.step(&mut human);
            if world.meals() > 0 {
                break;
            }
        }
        assert_eq!(world.meals(), 1);
    }

    #[test]
    fn a_stranger_never_goes_into_somebody_else_s_house_nor_eats_from_their_fridge() {
        let (mut world, mut human) = a_house_with_a_fridge(false);
        let routes = walker::ROUTES_ASKED.with(|asked| asked.get());
        for _ in 0..4000 {
            world.step(&mut human);
            let cell = human.center_position();
            assert_eq!(world.properties.of(cell), None, "a stranger at {cell:?}");
        }
        assert_eq!(world.meals(), 0, "the only fridge is somebody else's");
        // Not even tried for: it is not there for a stranger to choose, so no
        // walk to it fails.
        assert!(!world.log_contains("gave up on reaching"), "{:?}", world.lines());
        assert!(world.doors.iter().all(|(_, door)| door.openness() == 0.0), "nor opened their door");
        let asked = walker::ROUTES_ASKED.with(|asked| asked.get()) - routes;
        assert!(asked < 400, "{asked} routes in 4000 ticks");
    }

    #[test]
    fn in_a_district_everybody_holds_the_key_to_their_own_house_and_stays_out_of_the_others() {
        let district = crate::map::district::generate(7);
        let mut state = GameState::new(district.map.clone(), 7);
        state.spawn_from_spawners();
        assert_eq!(state.properties().len(), district.houses.len(), "every house is a property");

        for house in &district.houses {
            let home = state.properties().of(house.beds[0]).expect("a bed in a house is in its property");
            for (cell, door) in state.doors().iter() {
                if house.walls.contains(cell) {
                    assert_eq!(door.lock(), Some(home), "the house door locks the house");
                }
            }
            for &bed in &house.beds {
                assert_eq!(state.properties().of(bed), Some(home));
                let owner = state.homes().owner(bed).expect("everybody has a bed");
                assert_eq!(state.entities().get(owner).unwrap().body().home(), Some(home));
            }
        }
        for (cell, door) in state.doors().iter() {
            let in_a_house = district.houses.iter().any(|house| house.walls.contains(cell));
            assert_eq!(door.lock().is_some(), in_a_house, "{cell:?}: only a house's door is locked");
        }

        let mut opened = 0;
        for _ in 0..1500 {
            run(&mut state, 1);
            for entity in state.entities().iter() {
                let cell = entity.center_position();
                let owner = state.properties().of(cell);
                assert!(
                    owner.is_none() || owner == entity.body().home(),
                    "{} is in {owner:?} at {cell:?} and lives in {:?}",
                    entity.uid(),
                    entity.body().home()
                );
            }
            opened = opened.max(state.doors().iter().filter(|(_, door)| door.openness() > 0.0).count());
        }
        assert!(opened > 0, "nobody went in or out of anywhere all day");
    }

    #[test]
    fn determinism_survives_a_crowd_going_through_doors() {
        let build = || {
            let mut state = GameState::new(crate::map::district::generate(5).map, 5);
            state.spawn_from_spawners();
            // Past `PARALLEL_AT`, so the rounds that can run on threads do.
            for y in 1..20 {
                state.spawn(EntityType::Human, Point::new(1, y));
            }
            assert!(state.len() >= PARALLEL_AT);
            state
        };
        let (mut a, mut b) = (build(), build());
        run(&mut a, 900);
        run(&mut b, 900);
        let snapshot = |state: &GameState| {
            let positions: Vec<(u64, (f32, f32))> =
                state.entities().iter().map(|e| (e.uid().raw(), e.position())).collect();
            let doors: Vec<(Point, f32)> = state.doors().iter().map(|(cell, door)| (cell, door.openness())).collect();
            (positions, doors)
        };
        assert_eq!(snapshot(&a), snapshot(&b));
    }

    #[test]
    fn a_route_goes_through_a_house_door_only_for_somebody_holding_its_key() {
        let world = testing::World::new(property::tests::house());
        let inside = Point::new(5, 5);
        let home = world.properties.of(inside);
        let mut walk = Walker::new(Uid::new(EntityType::Human, 5), Point::new(5, 0), 2.0);
        assert!(!walk.route_to(&world.ctx(), inside), "a stranger has no way in");
        walk.body_mut().set_home(home);
        assert!(walk.route_to(&world.ctx(), inside), "its owner does");
        assert!(walk.path_cells().contains(&Point::new(5, 2)) || walk.path_cells().contains(&Point::new(4, 2)));
    }

    /// A small site in the wasteland of seed 3: the scrap of a yard out past
    /// its clearing lies a few dozen cells off, inside the navigation area.
    fn scrap_site() -> Map {
        Map::new(Size::new(8, 8), FLOOR).with_wasteland(3)
    }

    /// The heaps holding `pieces` nearest `from`, nearest first, each with
    /// open wasteland beside it to work it from.
    fn heaps_near(map: &Map, from: Point, pieces: u8) -> Vec<Point> {
        let mut heaps = Vec::new();
        map.heaps(crate::map::Bounds::from_corners(from.offset(-60, -60), from.offset(60, 60)), |cell, heap| {
            if heap.pieces == pieces {
                heaps.push(cell);
            }
        });
        heaps.retain(|heap| {
            heap.cardinal_neighbours().iter().any(|&side| map.is_passable(side) && map.navigation().is_outside(side))
        });
        heaps.sort_by_key(|heap| (heap.manhattan_distance(from), *heap));
        assert!(!heaps.is_empty(), "no heap of {pieces} near {from:?}");
        heaps
    }

    /// Spawn a human at `cell` with every need met, so for the length of a
    /// test nothing but the order is wanted.
    fn calm_human(state: &mut GameState, cell: Point) -> Uid {
        let uid = state.spawn(EntityType::Human, cell);
        let biology = state.entities.get_mut(uid).unwrap().biology_mut().unwrap();
        biology.edit(|_| {
            crate::sim::biology::Stats::calm()
                .with_hunger(0.0)
                .with_thirst(0.0)
                .with_bladder(0.0)
                .with_fun(100.0)
                .with_stamina(100.0)
        });
        uid
    }

    fn stowed_scrap(state: &GameState, uid: Uid) -> u8 {
        let inventory = state.entities().get(uid).unwrap().inventory().unwrap();
        inventory.stowed().find(|&(kind, _)| kind == ItemKind::Scrap).map_or(0, |(_, count)| count)
    }

    #[test]
    fn a_unit_told_to_salvage_a_heap_works_it_until_it_is_empty_and_carries_the_scrap_away() {
        let map = scrap_site();
        let heap = heaps_near(&map, Point::new(4, 4), 3)[0];
        let start = dry_cells(&map, heap.offset(-6, 0), 1)[0];
        let mut state = GameState::new(map, 1);
        let uid = calm_human(&mut state, start);
        assert!(!state.map.is_passable(heap), "a heap is in the way");

        let mut input = Input::new();
        input.salvage(uid, heap);
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let mut lines = state.log.drain();
        assert!(lines.iter().any(|l| l.contains("was told to salvage the scrap") && l.contains("3 left")), "{lines:?}");
        assert_eq!(state.entities().get(uid).unwrap().current_goal(), Some(GoalId::Salvage));

        // Three pieces at ten seconds of work each, and the walk there.
        for _ in 0..3600 {
            run(&mut state, 1);
            lines.extend(state.log.drain());
            if lines.iter().any(|l| l.contains("stopped salvaging")) {
                break;
            }
        }
        let salvaged = lines.iter().filter(|l| l.contains("salvaged scrap metal")).count();
        assert_eq!(salvaged, 3, "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("the heap is gone")));
        assert!(lines.iter().any(|l| l.contains("stopped salvaging") && l.contains("nothing left")), "{lines:?}");
        assert_eq!(stowed_scrap(&state, uid), 3);
        assert_eq!(state.map.scrap(heap), None);
        assert!(state.map.is_passable(heap), "cleared away, and walkable");
        let here = state.entities().get(uid).unwrap().center_position();
        assert_eq!(here.manhattan_distance(heap), 1, "worked from beside it");

        run(&mut state, 2);
        assert_ne!(state.entities().get(uid).unwrap().current_goal(), Some(GoalId::Salvage), "the order is done with");
    }

    #[test]
    fn a_unit_that_can_carry_no_more_lets_the_order_go() {
        let map = scrap_site();
        let heap = heaps_near(&map, Point::new(4, 4), 2)[0];
        let side = *heap.cardinal_neighbours().iter().find(|&&side| map.is_passable(side)).unwrap();
        let mut state = GameState::new(map, 2);
        let uid = calm_human(&mut state, side);
        // Two pieces in the pack already: room for one more, not two.
        assert!(state.give_item(uid, ItemKind::Scrap) && state.give_item(uid, ItemKind::Scrap));
        assert!(state.order_salvage(uid, heap));
        let mut lines = Vec::new();
        for _ in 0..2400 {
            run(&mut state, 1);
            lines.extend(state.log.drain());
        }
        assert!(lines.iter().any(|l| l.contains("stopped salvaging") && l.contains("can carry no more")), "{lines:?}");
        assert_eq!(stowed_scrap(&state, uid), 3);
        assert_eq!(state.map.scrap(heap).map(|h| h.pieces), Some(1), "one left for somebody else");
        assert!(!state.map.is_passable(heap));
    }

    #[test]
    fn the_last_piece_of_a_heap_goes_to_one_of_two_finishing_on_it_together() {
        let map = scrap_site();
        let (heap, sides) = heaps_near(&map, Point::new(4, 4), 1)
            .into_iter()
            .find_map(|heap| {
                let sides: Vec<Point> = heap.cardinal_neighbours().into_iter().filter(|&side| map.is_passable(side)).collect();
                (sides.len() >= 2).then_some((heap, sides))
            })
            .expect("a heap of one with two sides free");
        let mut state = GameState::new(map, 3);
        let (a, b) = (calm_human(&mut state, sides[0]), calm_human(&mut state, sides[1]));
        let mut input = Input::new();
        input.salvage(a, heap).salvage(b, heap);
        process_game_state(&mut state, 1.0 / 60.0, &input);
        let mut lines = Vec::new();
        for _ in 0..1200 {
            run(&mut state, 1);
            lines.extend(state.log.drain());
        }
        assert_eq!(stowed_scrap(&state, a) + stowed_scrap(&state, b), 1, "{lines:?}");
        assert_eq!(lines.iter().filter(|l| l.contains("salvaged scrap metal")).count(), 1);
        assert_eq!(state.map.scrap(heap), None);
        // Both worked it to the end, and the world settled who got it.
        assert!(lines.iter().any(|l| l.contains("found no scrap left")), "{lines:?}");
    }

    #[test]
    fn an_order_to_salvage_nothing_or_given_to_a_dog_is_refused() {
        let map = scrap_site();
        let heap = heaps_near(&map, Point::new(4, 4), 1)[0];
        let mut state = GameState::new(map, 4);
        let dog = state.spawn(EntityType::Dog, Point::new(2, 2));
        let human = state.spawn(EntityType::Human, Point::new(3, 3));
        assert!(!state.order_salvage(human, Point::new(-5, 3)), "nothing there");
        assert!(!state.order_salvage(dog, heap));
        let lines = state.log.drain();
        assert!(lines.iter().any(|l| l.contains("salvage: no scrap at -5, 3")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("cannot salvage")), "{lines:?}");
    }

    /// More scrap collectors than the parallel threshold, each with a bin of
    /// its own, all after the same yard: who gets which piece and which heap
    /// is next is settled by the world step and the order of the slots, and
    /// has to come out the same twice — down to what each bin holds.
    #[test]
    fn determinism_survives_a_crowd_of_scrap_collectors_in_the_parallel_rounds() {
        let build = || {
            let mut map = Map::new(Size::new(24, 24), FLOOR).with_wasteland(3);
            // Rows of bins with a free row between, one a collector.
            let bins: Vec<Point> = (0..PARALLEL_AT as i32 + 6).map(|i| Point::new(1 + i % 22, 1 + 2 * (i / 22))).collect();
            for &bin in &bins {
                crate::sim::testing::prop_at(&mut map, "scrap bin", bin);
            }
            let cells = dry_cells(&map, Point::new(-30, 4), bins.len());
            let mut state = GameState::new(map, 13);
            for &cell in &cells {
                calm_human(&mut state, cell);
            }
            state
        };
        let (mut a, mut b) = (build(), build());
        assert!(a.len() > PARALLEL_AT);
        let collectors = a.entities().iter().filter(|e| e.job().is_some_and(|job| job.kind == JobKind::ScrapCollector)).count();
        assert!(collectors > PARALLEL_AT, "{collectors} collectors");
        let count = |lines: Vec<String>| lines.iter().filter(|l| l.contains("salvaged scrap metal")).count();
        let (mut salvaged_a, mut salvaged_b) = (0, 0);
        // From eight until a quarter past eleven, at `run`'s 1800 ticks a
        // world hour.
        for _ in 0..(3.25 * 1800.0) as u32 {
            run(&mut a, 1);
            run(&mut b, 1);
            salvaged_a += count(a.log.drain());
            salvaged_b += count(b.log.drain());
        }
        let snapshot = |state: &GameState| {
            let positions: Vec<(u64, (f32, f32))> =
                state.entities().iter().map(|e| (e.uid().raw(), e.position())).collect();
            let carried: Vec<u8> = state.entities().iter().map(|e| stowed_scrap(state, e.uid())).collect();
            let bins: Vec<(Point, u32)> = state.bins().iter().collect();
            (positions, carried, bins)
        };
        assert!(salvaged_a > 0, "nobody salvaged anything, so the collecting never ran");
        assert!(a.bins().total() > 0, "nothing was stored, so the deposits never ran");
        assert_eq!(salvaged_a, salvaged_b);
        assert_eq!(snapshot(&a), snapshot(&b));
    }

    #[test]
    fn determinism_survives_a_crowd_salvaging_a_scrapyard() {
        // More people than the parallel threshold, sent at the heaps of a
        // yard in turns, several to a heap: who gets which piece is settled
        // by the world step, and has to be settled the same way twice.
        let build = || {
            let map = scrap_site();
            let heaps: Vec<Point> = (1..=3).flat_map(|pieces| heaps_near(&map, Point::new(4, 4), pieces)).take(12).collect();
            let cells = dry_cells(&map, heaps[0], PARALLEL_AT + 6);
            let mut state = GameState::new(map, 9);
            let mut input = Input::new();
            for (i, &cell) in cells.iter().enumerate() {
                let uid = calm_human(&mut state, cell);
                input.salvage(uid, heaps[i % heaps.len()]);
            }
            process_game_state(&mut state, 1.0 / 60.0, &input);
            (state, heaps)
        };
        let (mut a, heaps) = build();
        let (mut b, _) = build();
        assert!(a.len() > PARALLEL_AT);
        let (mut salvaged_a, mut salvaged_b) = (0, 0);
        let count = |lines: Vec<String>| lines.iter().filter(|l| l.contains("salvaged scrap metal")).count();
        for _ in 0..2400 {
            run(&mut a, 1);
            run(&mut b, 1);
            salvaged_a += count(a.log.drain());
            salvaged_b += count(b.log.drain());
        }
        let snapshot = |state: &GameState| {
            let positions: Vec<(u64, (f32, f32))> =
                state.entities().iter().map(|e| (e.uid().raw(), e.position())).collect();
            let carried: Vec<u8> = state.entities().iter().map(|e| stowed_scrap(state, e.uid())).collect();
            let left: Vec<Option<u8>> = heaps.iter().map(|&heap| state.map.scrap(heap).map(|h| h.pieces)).collect();
            (positions, carried, left)
        };
        assert!(salvaged_a > 0, "nobody salvaged anything, so the salvaging never ran");
        assert_eq!(salvaged_a, salvaged_b);
        assert_eq!(snapshot(&a), snapshot(&b));
    }
}

#[cfg(test)]
mod steel_probe {
    use super::*;
    use std::collections::BTreeMap;

    /// Where the scrap collectors', couriers' and steelworkers' shifts go,
    /// over `PROBE_DAYS` days with every need on.
    #[test]
    #[ignore]
    fn probe_the_scrap_and_steel_chain() {
        let path = std::env::var("PROBE_MAP").unwrap_or_else(|_| "seed:7".into());
        let days: u32 = std::env::var("PROBE_DAYS").ok().and_then(|d| d.parse().ok()).unwrap_or(5);
        let show = std::env::var("PROBE_LOG").ok();
        let map = match path.strip_prefix("seed:") {
            Some(seed) => crate::map::district::generate(seed.parse().unwrap()).map,
            None => Map::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap(),
        };
        let mut state = GameState::new(map, 0x5eed_c0de);
        state.spawn_from_spawners();
        let input = Input::new();
        let kinds = [job::JobKind::ScrapCollector, job::JobKind::Courier, job::JobKind::Steelworker];
        let workers: Vec<(Uid, job::JobKind)> = state
            .entities()
            .iter()
            .filter_map(|e| e.job().filter(|job| kinds.contains(&job.kind)).map(|job| (e.uid(), job.kind)))
            .collect();
        eprintln!("{} residents, {} chain workers", state.len(), workers.len());
        let mut time: BTreeMap<(String, String), u32> = Default::default();
        let mut lines: BTreeMap<(String, String), u32> = Default::default();
        let stores = |state: &GameState| {
            let bins = state.bins();
            format!(
                "bins {} hoppers {} pallets {} rack {}",
                bins.total_in(feature::FeatureKind::ScrapBin),
                bins.total_in(feature::FeatureKind::ScrapHopper),
                bins.total_in(feature::FeatureKind::SteelPallet),
                bins.total_in(feature::FeatureKind::SteelRack)
            )
        };
        for tick in 0..days * 24 * 1800 {
            process_game_state(&mut state, 1.0 / 60.0, &input);
            for line in state.log.drain() {
                let Some(&(uid, kind)) = workers.iter().find(|(u, _)| line.starts_with(&format!("{u} "))) else {
                    continue;
                };
                let words: Vec<&str> = line.split(' ').skip(1).take(3).filter(|w| !w.chars().any(|c| c.is_ascii_digit())).collect();
                *lines.entry((kind.name().to_string(), words.join(" "))).or_default() += 1;
                if show.as_deref() == Some(kind.name()) || show.as_deref() == Some(&uid.to_string()) {
                    eprintln!("  {} {line}", state.clock());
                }
            }
            if tick % 150 == 0 {
                let clock = state.clock();
                for &(uid, kind) in &workers {
                    let e = state.entities().get(uid).unwrap();
                    let what = if e.is_dead() {
                        "dead".to_string()
                    } else if !e.job().is_some_and(|j| j.is_due(clock)) {
                        continue;
                    } else {
                        let goal = e.current_goal().map(|g| g.name()).unwrap_or("none");
                        let fields = e.brain_fields();
                        let task = fields
                            .iter()
                            .find(|(k, _)| *k == "task")
                            .map(|(_, v)| v.split(' ').next().unwrap_or("").to_string())
                            .unwrap_or_default();
                        let out = if state.map.contains(e.body().center_position()) { "" } else { " OUT" };
                        format!("{goal} {task}{out}")
                    };
                    if std::env::var("PROBE_WHO").ok().as_deref() == Some(&uid.to_string()) {
                        let e = state.entities().get(uid).unwrap();
                        let fields = e.brain_fields();
                        let pick = |name: &str| fields.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone()).unwrap_or_default();
                        let s = *e.biology().unwrap().stats();
                        let pack = e.inventory().map(|i| i.count(item::ItemKind::Scrap)).unwrap_or(0);
                        eprintln!(
                            "    {clock} {what:24} at {:?} h{:.0} t{:.0} b{:.0} f{:.0} z{:.0} pack {pack} | {} | {}",
                            e.body().center_position(), s.hunger(), s.thirst(), s.bladder(), s.boredom(), s.tiredness(),
                            pick("task"), e.debug_fields().iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")
                        );
                    }
                    *time.entry((kind.name().to_string(), what)).or_default() += 1;
                }
            }
            if tick % (24 * 1800) == 9 * 1800 {
                eprintln!("{} {}", state.clock(), stores(&state));
            }
        }
        eprintln!("END {} {}", state.clock(), stores(&state));
        for kind in kinds {
            let total: u32 = time.iter().filter(|((k, _), _)| k == kind.name()).map(|(_, n)| n).sum();
            eprintln!("== {} ({} samples of 5 min on shift)", kind.name(), total);
            let mut rows: Vec<_> = time.iter().filter(|((k, _), _)| k == kind.name()).collect();
            rows.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
            for ((_, what), n) in rows.iter().take(14) {
                eprintln!("   {:5.1}%  {what}", **n as f32 * 100.0 / total as f32);
            }
            for ((_, verb), n) in lines.iter().filter(|((k, _), _)| k == kind.name()) {
                eprintln!("   log {n:5}  {verb}");
            }
        }
    }
}

#[cfg(test)]
mod economy_tests;
