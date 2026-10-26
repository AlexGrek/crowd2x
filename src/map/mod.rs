//! The map: the static world the crowd walks around in.
//!
//! **Nothing in this module imports `bevy`, and nothing should start to.** The
//! map is simulation state, so it is plain Rust: testable with `cargo test`
//! and no `App`, parallelisable on its own terms rather than through the ECS
//! scheduler, and insulated from the next Bevy release. The Bevy side reads it
//! and spawns sprites; it does not live in it.
//!
//! # Format
//!
//! A map is dimensions, then layers, in two kinds:
//!
//! * **Terrain** — any number of layers, every cell of every one of them
//!   defined. There is no "empty" cell, only the [`VOID`] tile, so no consumer
//!   has to handle a hole. Only the base layer exists so far; see
//!   [`Map::terrain_layers`].
//! * **Objects** — sparse lists of positioned things, one list per
//!   [`ObjectLayer`]: props, spawners and ceiling lamps. A prop blocks the cell it stands in
//!   ([`PROPS`]) unless it hangs on a wall beside it ([`mount`]: a light
//!   switch); the simulation reads props for what they are *for*
//!   (`sim::feature` — a fridge is food), by name. A spawner names an entity
//!   kind (`"human"`), and playing the map spawns one of it where it stands
//!   (`sim::GameState::spawn_from_spawners`).
//!
//! * **Grid layers** — a small value per cell, authored, one layer per
//!   [`GridLayer`]: the ceiling (whether there is a roof over a cell, which
//!   is what keeps the sun out — `crate::lighting`), and under the floor the
//!   electrical grid and the sewer pipes, which decide whether a fridge has
//!   power and a toilet a drain ([`utilities`]). None of them is in anybody's
//!   way, and a map that never painted one — every map made before it
//!   existed — has nothing in it: open sky, no wiring, no pipes.
//!
//! Maps serialise to JSON — see [`format`] for the on-disk shape, and
//! [`Map::to_json`] / [`Map::from_json`].
//!
//! Passability is *derived*, not authored: it is rebuilt from the terrain and
//! the props, and kept in sync by [`Map::set_terrain`], [`Map::add_object`]
//! and [`Map::remove_object`], so a map cannot end up claiming a wall or a
//! fridge can be walked through. See [`PassabilityMap`] for why it is a
//! separate structure at all.
//!
//! # The site and the world round it
//!
//! Everything above is the **site**: the finite rectangle from `(0, 0)` to
//! [`Map::size`] that was authored, saved and edited. A map from a file, the
//! district generator or the browser does not stop at its edge — it is
//! surrounded by the endless [`wasteland`], dirt and scrub and lakes running
//! off in every direction, which is terrain and heaps of scrap metal and
//! nothing else: no props, no roof, no wiring. [`Map::terrain`], [`Map::is_passable`],
//! [`Map::is_see_through`] and [`Map::walk_cost`] answer for any cell in the
//! world; the site's own layers and everything derived from them (the supply,
//! the features, the lightmap) stay the size of the site, there being nothing
//! out there for them. The wasteland is a pure function of a seed, cached a
//! [`chunk`] at a time ([`Map::load_chunks`]) by whatever reads it often — a
//! camera, the simulation — and loading never changes what a cell is.
//!
//! **Scrap is the one part of the world play changes.** A heap
//! ([`Map::scrap`]) is in the way until it has been emptied, a piece at a
//! time ([`Map::take_scrap`]); what has been taken from which heap is kept
//! here, beside the chunks, so a chunk dropped and loaded again still has
//! its emptied heaps gone. It is the simulation's state, never a map file's:
//! a map is saved with every heap as it was found, and only the simulation's
//! own copy of a map is ever salvaged. A game's save keeps it, with the
//! loaded chunks, chunk by chunk ([`save`]).
//!
//! **And crops.** A farmer sows wheat in good soil (`Terrain::fertile`), one
//! plant to a [plot](is_plot), and a plant is in the way like a heap until it
//! is harvested ([`Map::plant`], [`Map::unplant`]). Which cells are planted
//! is kept here because passability is; *what* grows there — whose it is,
//! how old, how well kept — is the simulation's (`sim::fields`), which plants
//! the map again when a game is loaded.
//!
//! A map built in code ([`Map::new`]) has no wasteland: off the site is
//! nothing, impassable and opaque, which is the bounded box every unit test
//! was written against. [`Map::with_wasteland`] gives it one.

// The map is the foundation for a simulation that does not exist yet, so most
// of it is reached only from tests until there is something to walk around on
// it. In a binary crate that makes the public surface, re-exports included,
// look unused; the alternative is trimming the API down to whatever today's
// callers happen to touch.
#![allow(dead_code, unused_imports)]

pub mod chunk;
mod coords;
pub mod district;
mod format;
pub mod grid;
pub mod landmarks;
pub mod mount;
mod passability;
mod save;
mod navigation;
pub use navigation::Navigation;
mod props;
mod storage;
mod terrain;
pub mod utilities;
pub mod wasteland;

pub use chunk::{Chunk, ChunkGrid, ChunkPos, CHUNK};
pub use coords::{Bounds, Point, Size};
pub use wasteland::{Scrap, Wasteland};
pub use format::{MapFormatError, FORMAT_VERSION};
pub use grid::GridLayer;
pub use passability::PassabilityMap;
pub use props::{Prop, PROPS};
pub use storage::{sanitize_name, MapStore, StorageError, MAX_NAME};
pub(crate) use storage::write_atomic;
pub use terrain::{
    Passability, Terrain, TerrainId, DIRT, DIRT_CRACKED, DIRT_STONES, FLOOR, MEADOW, MIN_WALK_COST, MUD, SCRUB,
    TERRAIN, VOID, WALL, WATER,
};

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// How many whole pixels wide a cell is, which is what an [`Object::at`] is
/// measured in.
///
/// Here rather than beside the art because the map format already commits to
/// pixel coordinates for objects, so converting one to a cell is a fact about
/// the format — and the simulation, which may not import the Bevy side where
/// the tile size used to live, needs to do it. `editor::background::TILE` is
/// defined from this, so the two cannot drift.
pub const PIXELS_PER_CELL: i32 = 48;

/// Stock a store receives when a new game starts. Cells without a store
/// ignore their entry, so deleting a store in the editor also removes its supply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialStock {
    pub cell: Point,
    pub pieces: u32,
}

/// The terrain layer every map has. Overlay layers, when they arrive, are
/// indices above this one.
pub const BASE: usize = 0;

/// How far past the site's edge reachability is worked out cell by cell
/// ([`Navigation`]), and so how far a route can swing out into the wasteland
/// round an obstacle before a search's window would have to grow. Two
/// chunks: a lake near a district fits inside it.
pub const NAV_MARGIN: i32 = 2 * CHUNK;

/// How far from the origin, in cells, the world is guaranteed to behave.
///
/// The wasteland has no end, and cells are `i32`, but positions are `f32` —
/// a unit's in cells, a sprite's and the camera's in canvas pixels — and an
/// `f32` loses precision as it grows: fifty thousand cells out a camera
/// position is still good to a quarter of a pixel and a unit's to under a
/// hundredth of a cell, ten times that and a walk's per-tick step starts to
/// round away. So the camera stops here (`game::move_camera`) — four hours of
/// panning at the default zoom, which is endless in every way that matters.
pub const WORLD_REACH: i32 = 50_000;

/// One terrain layer: exactly one tile per cell, row-major.
#[derive(Clone)]
pub struct TerrainLayer {
    tiles: Vec<TerrainId>,
}

impl TerrainLayer {
    /// Row-major, `size.area()` long — index it with [`Size::index_of`].
    pub fn tiles(&self) -> &[TerrainId] {
        &self.tiles
    }
}

/// Which list an object belongs to.
///
/// The discriminants are the storage order in [`Map`], so a layer is looked up
/// by casting rather than by searching.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectLayer {
    Props = 0,
    Spawners = 1,
    /// Lights hung from the ceiling. Up out of everybody's way, so a lamp
    /// never blocks a cell, and what it is for is light alone
    /// (`crate::lighting::scene::EMITTERS`, by name).
    Lamps = 2,
}

impl ObjectLayer {
    pub const ALL: [ObjectLayer; 3] = [ObjectLayer::Props, ObjectLayer::Spawners, ObjectLayer::Lamps];

    /// The key this layer is stored under in a map file. Renaming one is a
    /// format change, not a cosmetic edit.
    pub const fn name(self) -> &'static str {
        match self {
            ObjectLayer::Props => "props",
            ObjectLayer::Spawners => "spawners",
            ObjectLayer::Lamps => "lamps",
        }
    }

    pub fn from_name(name: &str) -> Option<ObjectLayer> {
        ObjectLayer::ALL.into_iter().find(|layer| layer.name() == name)
    }
}

/// What an object is, by name — `"bed 1"`, `"crate"`.
///
/// A name for the same reason terrain uses one: the catalogue an id would
/// index into lives in the editor, next to the art, and reordering it must not
/// silently turn every saved bed into a toilet. Whoever owns the layer
/// resolves the name; a spawner and a prop will not share a catalogue.
///
/// A spawner's kind is an entity kind (`"human"`): the game spawns one of
/// those in the spawner's cell when the map is played. Only the district
/// generator ([`district`]) writes spawners so far; the editor has no tool
/// for them, and keeps the ones a map already has.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[derive(Serialize, Deserialize)]
pub struct ObjectKind(pub String);

impl ObjectKind {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A placed object.
#[derive(Clone, PartialEq, Eq, Debug)]
#[derive(Serialize, Deserialize)]
pub struct Object {
    /// World position in **whole pixels**, not a cell.
    ///
    /// Object layers are free placement — that is the entire difference
    /// between them and the terrain grid. A prop is depth-sorted against the
    /// crowd by its world Y, so rounding it to a cell would both move it and
    /// throw away the ordering. Still `i32`: the renderer cannot draw on a
    /// fractional pixel either.
    pub at: Point,
    pub kind: ObjectKind,
}

impl Object {
    /// The cell the object's position falls in.
    ///
    /// Floored, like every other position-to-cell rule in the game, so an
    /// object half a pixel left of the origin is in cell -1 and not cell 0.
    pub fn cell(&self) -> Point {
        Point::new(
            self.at.x.div_euclid(PIXELS_PER_CELL),
            self.at.y.div_euclid(PIXELS_PER_CELL),
        )
    }
}

/// Deliberately summarised rather than derived: a derived `Debug` on a map
/// prints every cell, which in a test failure buries the assertion that
/// actually matters under a few thousand tile ids.
impl std::fmt::Debug for Map {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Map")
            .field("size", &self.size)
            .field("wasteland", &self.wasteland.map(Wasteland::seed))
            .field("loaded_chunks", &self.chunks.len())
            .field("salvaged", &self.salvaged.len())
            .field("planted", &self.planted.len())
            .field("terrain_layers", &self.terrain.len())
            .field("props", &self.objects(ObjectLayer::Props).len())
            .field("spawners", &self.objects(ObjectLayer::Spawners).len())
            .field("lamps", &self.objects(ObjectLayer::Lamps).len())
            .field("ceiling", &self.ceiling_count())
            .field("power", &self.grid_count(GridLayer::Power))
            .field("water", &self.grid_count(GridLayer::Water))
            .finish()
    }
}

/// `Clone` because the simulation owns its own copy: [`crate::sim::GameState`]
/// outlives whatever loaded the map, and the editor keeps editing the original
/// meanwhile.
#[derive(Clone)]
pub struct Map {
    size: Size,
    /// At least one; `terrain[BASE]` is the ground.
    terrain: Vec<TerrainLayer>,
    /// Indexed by `ObjectLayer as usize`, one entry per variant.
    objects: Vec<Vec<Object>>,
    initial_stock: Vec<InitialStock>,
    passability: PassabilityMap,
    navigation: std::sync::OnceLock<Navigation>,
    navigation_revision: u64,
    /// Which cells can be seen *through*: the terrain's half of passability
    /// alone — a wall hides what is behind it, a fridge or a bed does not.
    /// The same one-bit-per-cell shape, for the same reason: a unit looking
    /// round asks it about every cell on a line of sight, and the catalogue
    /// behind a tile id is a lookup and a branch per cell where this is a bit.
    sight: PassabilityMap,
    /// One row-major byte per cell per [`GridLayer`], indexed by
    /// `GridLayer as usize`: the ceiling, the wiring, the pipes. Authored,
    /// like the terrain, and in nobody's way.
    grids: Vec<Vec<u8>>,
    /// What is off the site: the endless wasteland, or `None` — nothing at
    /// all, which is what a map built in code starts with.
    wasteland: Option<Wasteland>,
    /// The wasteland, a chunk at a time, for whoever reads it often. A
    /// cache: a cell reads the same loaded or not.
    chunks: ChunkGrid<Box<Chunk>>,
    /// How many pieces have been taken from each heap of scrap anybody has
    /// been at, by chunk and cell in it — so one chunk's heaps are a range.
    /// Simulation state: never in a map file (see the module docs), but kept
    /// by a game's save, chunk by chunk (`map::save`).
    salvaged: BTreeMap<(ChunkPos, u16), u8>,
    /// Bumped by every piece taken, so whatever draws the heaps knows to look
    /// again.
    salvage_revision: u64,
    /// Every cell a crop stands in, on the site or off it, by chunk and cell
    /// in it like `salvaged`. In the way until harvested ([`Map::plant`]).
    /// Simulation state the simulation keeps the rest of (`sim::fields`): not
    /// in a map file, nor in a save's chunks — the save's crops plant it again.
    planted: BTreeSet<(ChunkPos, u16)>,
}

/// Where a heap's tally is kept in `Map::salvaged`, and a crop in
/// `Map::planted`.
fn salvage_key(cell: Point) -> (ChunkPos, u16) {
    (ChunkPos::of(cell), chunk::local_index(cell) as u16)
}

/// The cell a [`salvage_key`] is for.
fn cell_of_key(pos: ChunkPos, index: u16) -> Point {
    pos.origin().offset(index as i32 % CHUNK, index as i32 / CHUNK)
}

/// Whether `cell` is a **plot**: one of the cells a crop may be sown in —
/// both its coordinates even. So no two crops are ever side by side or
/// corner to corner, the eight cells round a plot are cells no crop can
/// stand in, and on open ground every cell left between crops joins up with
/// every other: a field, however it is sown, never shuts anybody in or walls
/// a way off — the scrap's one heap to a two-by-two block (`wasteland`), for
/// crops. Where the ground is not open all round, [`Map::can_sow`] asks for
/// the ring round the plot to be.
pub const fn is_plot(cell: Point) -> bool {
    cell.x.rem_euclid(2) == 0 && cell.y.rem_euclid(2) == 0
}

/// Whether every cell of the chunk at `pos` is on the site, so it would
/// never be read.
fn chunk_is_all_site(pos: ChunkPos, site: Bounds) -> bool {
    let chunk = pos.bounds();
    site.contains(chunk.origin) && site.contains(chunk.max())
}

impl Map {
    /// New-game supplies, saved with the authored map, not changed by play.
    pub fn initial_stock(&self) -> &[InitialStock] {
        &self.initial_stock
    }

    /// Set a store's new-game supplies. Refuse cells outside the site.
    pub fn set_initial_stock(&mut self, cell: Point, pieces: u32) -> bool {
        if self.size.index_of(cell).is_none() { return false; }
        self.initial_stock.retain(|stock| stock.cell != cell);
        if pieces > 0 {
            self.initial_stock.push(InitialStock { cell, pieces });
            self.initial_stock.sort_unstable_by_key(|stock| stock.cell);
        }
        true
    }

    /// A map of `size` with its base terrain layer filled with `tile`, and
    /// nothing round it.
    pub fn new(size: Size, tile: TerrainId) -> Self {
        Self::from_base_layer(size, vec![tile; size.area()])
    }

    /// A map from a base layer that is already laid out row-major.
    ///
    /// The one place a map is built, so passability is derived exactly once
    /// and cannot be forgotten by a second constructor.
    fn from_base_layer(size: Size, tiles: Vec<TerrainId>) -> Self {
        debug_assert_eq!(tiles.len(), size.area());
        let mut map = Self {
            size,
            terrain: vec![TerrainLayer { tiles }],
            objects: ObjectLayer::ALL.map(|_| Vec::new()).into(),
            initial_stock: Vec::new(),
            passability: PassabilityMap::new(size),
            navigation: Default::default(),
            navigation_revision: 0,
            sight: PassabilityMap::new(size),
            grids: GridLayer::ALL.map(|_| vec![0; size.area()]).into(),
            wasteland: None,
            chunks: ChunkGrid::new(),
            salvaged: BTreeMap::new(),
            salvage_revision: 0,
            planted: BTreeSet::new(),
        };
        map.rebuild_passability();
        map
    }

    /// The same map, surrounded by the wasteland of `seed`.
    pub fn with_wasteland(mut self, seed: u64) -> Self {
        self.set_wasteland(Some(Wasteland::new(seed)));
        self
    }

    /// What is off the site: the wasteland, or nothing.
    pub fn wasteland(&self) -> Option<Wasteland> {
        self.wasteland
    }

    /// Surround the site with a wasteland, or with nothing. Forgets every
    /// loaded chunk, which was the other wasteland's ground, everything
    /// salvaged from its scrap and everything sown in its soil.
    pub fn set_wasteland(&mut self, wasteland: Option<Wasteland>) {
        if self.wasteland != wasteland {
            self.wasteland = wasteland;
            self.chunks = ChunkGrid::new();
            self.salvaged.clear();
            let site = self.site();
            self.planted.retain(|&(pos, index)| site.contains(cell_of_key(pos, index)));
            self.salvage_revision = self.salvage_revision.wrapping_add(1);
            self.invalidate_navigation();
        }
    }

    /// Whether the world goes on past the site.
    pub fn is_endless(&self) -> bool {
        self.wasteland.is_some()
    }

    /// The cells the site covers, as [`Bounds`].
    pub fn site(&self) -> Bounds {
        Bounds::from(self.size)
    }

    /// Where reachability is worked out cell by cell: the site, and on an
    /// endless map [`NAV_MARGIN`] of the wasteland round it. Past it, every
    /// cell counts as joined to everything that reaches its edge.
    pub fn navigation_area(&self) -> Bounds {
        if self.is_endless() { self.site().grown(NAV_MARGIN) } else { self.site() }
    }

    /// Load the chunk at `pos`, so its cells are read from memory rather
    /// than worked out again. Returns whether it was loaded now — not when it
    /// already was, the map has no wasteland, or the chunk is all site.
    pub fn load_chunk(&mut self, pos: ChunkPos) -> bool {
        let Some(wasteland) = self.wasteland else {
            return false;
        };
        let site = self.site();
        if self.chunks.contains(pos) || chunk_is_all_site(pos, site) {
            return false;
        }
        self.chunks.get_or_insert_with(pos, || {
            Chunk::generate(pos, |cell| wasteland.tile(cell, site), |cell, tile| wasteland.scrap_on(cell, site, tile))
        });
        self.clear_salvaged(pos);
        self.block_planted(pos);
        true
    }

    /// Close the cells of the loaded chunk at `pos` that have a crop in them:
    /// a field sown before the chunk was dropped is still there.
    fn block_planted(&mut self, pos: ChunkPos) {
        let Some(chunk) = self.chunks.get_mut(pos) else {
            return;
        };
        for &(_, index) in self.planted.range((pos, 0)..=(pos, u16::MAX)) {
            chunk.block(cell_of_key(pos, index));
        }
    }

    /// Open the cells of the loaded chunk at `pos` whose heaps have been
    /// emptied: a heap emptied before the chunk was dropped is still empty.
    fn clear_salvaged(&mut self, pos: ChunkPos) {
        let Some(chunk) = self.chunks.get_mut(pos) else {
            return;
        };
        let origin = pos.origin();
        for (&(_, index), &taken) in self.salvaged.range((pos, 0)..=(pos, u16::MAX)) {
            let cell = origin.offset(index as i32 % CHUNK, index as i32 / CHUNK);
            if chunk.scrap(cell).is_some_and(|found| found.pieces <= taken) {
                chunk.clear(cell);
            }
        }
    }

    /// Load every chunk with a cell in `area`; returns how many were loaded
    /// now.
    pub fn load_chunks(&mut self, area: Bounds) -> usize {
        ChunkPos::covering(area).filter(|&pos| self.load_chunk(pos)).count()
    }

    /// Drop every loaded chunk `keep` says no to; returns how many went.
    /// Nothing about any cell changes — they are worked out from the seed
    /// again until something loads them back.
    pub fn unload_chunks(&mut self, mut keep: impl FnMut(ChunkPos) -> bool) -> usize {
        self.chunks.retain(|pos, _| keep(pos))
    }

    /// [`Map::unload_chunks`] over at most `count` of the loaded chunks,
    /// starting at `cursor` and leaving it for next time — a bounded slice of
    /// the whole, for a caller that reclaims a little every frame
    /// ([`crate::map::ChunkGrid::sweep`]).
    pub fn sweep_chunks(&mut self, cursor: &mut usize, count: usize, mut keep: impl FnMut(ChunkPos) -> bool) -> usize {
        self.chunks.sweep(cursor, count, |pos, _| keep(pos))
    }

    /// How many chunks of wasteland are in memory.
    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_chunk_loaded(&self, pos: ChunkPos) -> bool {
        self.chunks.contains(pos)
    }

    /// The wasteland's tile at `cell`, off the site: the loaded chunk's, or
    /// the generator's. `None` with no wasteland.
    #[inline]
    fn ground(&self, cell: Point) -> Option<TerrainId> {
        let wasteland = self.wasteland?;
        Some(match self.chunks.at(cell) {
            Some(chunk) => chunk.tile(cell),
            None => wasteland.tile(cell, self.site()),
        })
    }

    /// Whether a cell off the site whose chunk is not loaded can be walked:
    /// its ground can, and no heap of scrap is left on it. What a loaded
    /// chunk's passable bit says, worked out.
    fn ground_is_passable(&self, cell: Point) -> bool {
        let Some(wasteland) = self.wasteland else {
            return false;
        };
        let site = self.site();
        let tile = wasteland.tile(cell, site);
        tile.is_passable()
            && wasteland.scrap_on(cell, site, tile).is_none_or(|found| self.left_of(cell, found).is_none())
            && !self.is_planted(cell)
    }

    /// What is left of a heap found as `found` in `cell`: `None` once it is
    /// empty.
    #[inline]
    fn left_of(&self, cell: Point, found: Scrap) -> Option<Scrap> {
        let taken = if self.salvaged.is_empty() {
            0
        } else {
            self.salvaged.get(&salvage_key(cell)).copied().unwrap_or(0)
        };
        (found.pieces > taken).then_some(Scrap { pieces: found.pieces - taken, ..found })
    }

    /// The heap of scrap in `cell`, with however many pieces are still in it
    /// — `None` where there is none, and where there was one that has been
    /// emptied. Never on the site, nor on a map with nothing round it.
    pub fn scrap(&self, cell: Point) -> Option<Scrap> {
        let wasteland = self.wasteland?;
        if self.contains(cell) {
            return None;
        }
        let found = match self.chunks.at(cell) {
            Some(chunk) => chunk.scrap(cell),
            None => wasteland.scrap(cell, self.site()),
        }?;
        self.left_of(cell, found)
    }

    /// Take one piece of scrap metal from the heap in `cell`, and say how
    /// many are left in it; `None`, and nothing taken, where there is no
    /// heap or nothing left in it. The last piece clears the heap away and
    /// opens its cell.
    ///
    /// The one edit play makes to a map (`sim::world_step`, for whoever
    /// finished salvaging it). It does not touch [`Map::navigation`]:
    /// reachability already counts a heap's cell as open, the way it counts
    /// a door ([`Map::is_open_ground`]), so clearing one changes nothing it
    /// said.
    pub fn take_scrap(&mut self, cell: Point) -> Option<u8> {
        let heap = self.scrap(cell)?;
        *self.salvaged.entry(salvage_key(cell)).or_insert(0) += 1;
        self.salvage_revision = self.salvage_revision.wrapping_add(1);
        let left = heap.pieces - 1;
        if left == 0
            && let Some(chunk) = self.chunks.get_mut(ChunkPos::of(cell))
        {
            chunk.clear(cell);
        }
        Some(left)
    }

    /// Bumped by every piece of scrap taken: what a drawing of the heaps
    /// compares to know it is out of date.
    pub fn salvage_revision(&self) -> u64 {
        self.salvage_revision
    }

    /// Every heap of scrap with anything left in it in `area`, chunk by
    /// chunk and bottom row first, with what is left of it. A loaded
    /// chunk's heaps are a list; a chunk not loaded is asked of the
    /// generator cell by cell, which is slower and the same answer.
    pub fn heaps(&self, area: Bounds, mut each: impl FnMut(Point, Scrap)) {
        let Some(wasteland) = self.wasteland else {
            return;
        };
        let site = self.site();
        for pos in ChunkPos::covering(area) {
            if chunk_is_all_site(pos, site) {
                continue;
            }
            match self.chunks.get(pos) {
                Some(chunk) => {
                    for (cell, found) in chunk.heaps(pos) {
                        if area.contains(cell)
                            && let Some(left) = self.left_of(cell, found)
                        {
                            each(cell, left);
                        }
                    }
                }
                None => {
                    for cell in pos.bounds().points().filter(|&cell| area.contains(cell)) {
                        if let Some(left) = wasteland.scrap(cell, site).and_then(|found| self.left_of(cell, found)) {
                            each(cell, left);
                        }
                    }
                }
            }
        }
    }

    /// Whether `point` could be walked once whatever scrap is on it has been
    /// cleared away: what reachability is worked out over ([`Navigation`]),
    /// which counts a heap as something that can go, the way it counts a
    /// door as something that can open.
    pub fn is_open_ground(&self, point: Point) -> bool {
        self.is_passable(point) || self.scrap(point).is_some() || self.is_planted(point)
    }

    /// Whether a crop could be sown in `cell` now: a [plot](is_plot) of good
    /// soil (`Terrain::fertile`) with nothing in the way — no crop, no heap,
    /// no prop — **and the eight cells round it open**. Who is standing
    /// there is not the map's to say.
    ///
    /// The ring is what keeps the promise [`is_plot`] makes where the ground
    /// is not open all round: a plot in a gap one cell wide — between a lake
    /// and a heap, against a wall — would close the gap. None of the eight is
    /// a plot, so no crop ever stands in them, and nothing else in play ever
    /// closes a cell (a heap only ever goes): with the ring open, whatever way
    /// ran through the plot runs round it, for as long as the crop stands.
    pub fn can_sow(&self, cell: Point) -> bool {
        is_plot(cell)
            && self.terrain(cell).is_some_and(TerrainId::is_fertile)
            && self.is_passable(cell)
            && (-1..=1).all(|dy| (-1..=1).all(|dx| self.is_passable(cell.offset(dx, dy))))
    }

    /// Whether a crop stands in `cell`.
    #[inline]
    pub fn is_planted(&self, cell: Point) -> bool {
        !self.planted.is_empty() && self.planted.contains(&salvage_key(cell))
    }

    /// Every cell a crop stands in, by chunk and cell in it.
    pub fn planted(&self) -> impl Iterator<Item = Point> + '_ {
        self.planted.iter().map(|&(pos, index)| cell_of_key(pos, index))
    }

    /// Sow `cell`: from now on it is in the way, as a heap is, until
    /// [`Map::unplant`]. Refused — `false`, nothing changed — where
    /// [`Map::can_sow`] says no.
    ///
    /// Like [`Map::take_scrap`], it leaves [`Map::navigation`] alone:
    /// reachability counts a crop's cell as open ground, the way it counts a
    /// heap, and a crop only ever stands in a plot, so no field can cut one
    /// cell off from another.
    pub fn plant(&mut self, cell: Point) -> bool {
        if !self.can_sow(cell) {
            return false;
        }
        self.planted.insert(salvage_key(cell));
        match self.size.index_of(cell) {
            Some(_) => self.passability.set(cell, false),
            None => {
                if let Some(chunk) = self.chunks.get_mut(ChunkPos::of(cell)) {
                    chunk.block(cell);
                }
            }
        }
        true
    }

    /// The crop in `cell` is gone — harvested — and the cell is open again.
    /// `false` where there was none.
    pub fn unplant(&mut self, cell: Point) -> bool {
        if !self.planted.remove(&salvage_key(cell)) {
            return false;
        }
        match self.size.index_of(cell) {
            // A crop is only sown on open ground, and nothing in play puts a
            // prop down: the cell is as open as it was.
            Some(_) => self.passability.set(cell, true),
            None => {
                if let Some(chunk) = self.chunks.get_mut(ChunkPos::of(cell)) {
                    chunk.clear(cell);
                }
            }
        }
        true
    }

    /// An unbuilt map: every cell [`VOID`], and so nothing walkable anywhere.
    pub fn empty(size: Size) -> Self {
        Self::new(size, VOID)
    }

    /// Built before simulation starts; map edits invalidate the derived index.
    pub fn navigation(&self) -> &Navigation {
        self.navigation.get_or_init(|| Navigation::build(self))
    }
    pub fn navigation_revision(&self) -> u64 {
        self.navigation_revision
    }
    fn invalidate_navigation(&mut self) {
        self.navigation.take();
        self.navigation_revision = self.navigation_revision.wrapping_add(1);
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Whether `point` is on the site — the authored part, which is all an
    /// edit can touch. The world round it may go on regardless
    /// ([`Map::is_endless`]).
    pub fn contains(&self, point: Point) -> bool {
        self.size.contains(point)
    }

    /// How many terrain layers there are. One, for now — the format allows
    /// more and the accessors below will need a layer argument when a second
    /// one turns up.
    pub fn terrain_layers(&self) -> usize {
        self.terrain.len()
    }

    pub fn base_layer(&self) -> &TerrainLayer {
        &self.terrain[BASE]
    }

    /// The base-layer tile at `point`: the site's own, the wasteland's off
    /// it, or `None` off the site of a map with no wasteland.
    #[inline]
    pub fn terrain(&self, point: Point) -> Option<TerrainId> {
        match self.size.index_of(point) {
            Some(index) => Some(self.terrain[BASE].tiles[index]),
            None => self.ground(point),
        }
    }

    /// Paint a base-layer cell, keeping the passability map in step. Returns
    /// whether the point was on the map.
    ///
    /// Floor painted under a fridge is still a fridge's cell: the cell is
    /// worked out again from everything that decides it, not from the tile.
    pub fn set_terrain(&mut self, point: Point, tile: TerrainId) -> bool {
        let Some(index) = self.size.index_of(point) else {
            return false;
        };
        self.terrain[BASE].tiles[index] = tile;
        self.refresh_passability(point);
        true
    }

    /// Recompute passability for the whole map.
    ///
    /// Only needed after a bulk change that bypassed the setters — loading a
    /// file, say. With [`Map::refresh_passability`] it is where the rule lives
    /// — **a cell is passable when its terrain is and no blocking prop stands
    /// in it** — so when overlay layers arrive (a rug over a floor) combining
    /// them happens here and every caller keeps asking the same question.
    pub fn rebuild_passability(&mut self) {
        self.invalidate_navigation();
        for (index, tile) in self.terrain[BASE].tiles.iter().enumerate() {
            let point = self.size.point_at(index);
            self.passability.set(point, tile.is_passable());
            self.sight.set(point, tile.is_see_through());
        }
        for prop in &self.objects[ObjectLayer::Props as usize] {
            if !prop.kind.prop_passability().is_passable() {
                self.passability.set(prop.cell(), false);
            }
        }
        for &(pos, index) in &self.planted {
            let cell = cell_of_key(pos, index);
            if self.size.contains(cell) {
                self.passability.set(cell, false);
            }
        }
    }

    /// Recompute passability for one cell: the rule of
    /// [`Map::rebuild_passability`], for the cell an edit touched.
    ///
    /// Scans the props, which is the cost of an edit and never of a tick —
    /// nothing in the simulation changes the map.
    fn refresh_passability(&mut self, point: Point) {
        self.invalidate_navigation();
        let terrain = self.terrain(point);
        self.sight.set(point, terrain.is_some_and(TerrainId::is_see_through));
        let passable = terrain.is_some_and(TerrainId::is_passable)
            && !self.is_planted(point)
            && !self.objects(ObjectLayer::Props).iter().any(|prop| {
                prop.cell() == point && !prop.kind.prop_passability().is_passable()
            });
        self.passability.set(point, passable);
    }

    /// What `layer` holds at `point` — `0`, nothing, off the map.
    pub fn grid(&self, layer: GridLayer, point: Point) -> u8 {
        self.size.index_of(point).map_or(0, |index| self.grids[layer as usize][index])
    }

    /// Row-major, `size.area()` long — index it with [`Size::index_of`].
    pub fn grid_cells(&self, layer: GridLayer) -> &[u8] {
        &self.grids[layer as usize]
    }

    /// Paint one cell of a grid layer. Returns whether the point was on the
    /// map and the value is one the layer has ([`GridLayer::alphabet`]);
    /// nothing else about the cell changes, since no grid layer is in
    /// anybody's way.
    pub fn set_grid(&mut self, layer: GridLayer, point: Point, value: u8) -> bool {
        let Some(index) = self.size.index_of(point) else {
            return false;
        };
        if layer.char_of(value).is_none() {
            return false;
        }
        self.grids[layer as usize][index] = value;
        true
    }

    /// How many cells of `layer` hold anything at all.
    pub fn grid_count(&self, layer: GridLayer) -> usize {
        self.grids[layer as usize].iter().filter(|&&value| value != 0).count()
    }

    /// Whether there is a ceiling over `point`. Off the map is open sky.
    pub fn has_ceiling(&self, point: Point) -> bool {
        self.grid(GridLayer::Ceiling, point) != grid::ceiling::OPEN
    }

    /// Roof a cell over, or open it to the sky. Returns whether the point was
    /// on the map. Nothing else about the cell changes: a ceiling is not in
    /// anybody's way.
    pub fn set_ceiling(&mut self, point: Point, roofed: bool) -> bool {
        let value = if roofed { grid::ceiling::ROOFED } else { grid::ceiling::OPEN };
        self.set_grid(GridLayer::Ceiling, point, value)
    }

    /// How many cells have a ceiling.
    pub fn ceiling_count(&self) -> usize {
        self.grid_count(GridLayer::Ceiling)
    }

    /// The site's static passability map, for a scan that has already
    /// worked out an index into it. Off the site it answers "impassable"
    /// whatever is there: ask [`Map::is_passable`] about the world.
    pub fn passability(&self) -> &PassabilityMap {
        &self.passability
    }

    /// Which of the site's cells can be seen through: see-through terrain
    /// (floor, a window), whatever stands on it. Like [`Map::passability`],
    /// the site only — [`Map::is_see_through`] answers for the world.
    pub fn sight(&self) -> &PassabilityMap {
        &self.sight
    }

    /// Whether an agent can stand at `point`: the terrain allows it and no
    /// prop is in the way. Off the site, whether the wasteland's ground
    /// there can be walked; with no wasteland, never.
    #[inline]
    pub fn is_passable(&self, point: Point) -> bool {
        match self.size.index_of(point) {
            Some(index) => self.passability.is_passable_at(index),
            None => match self.chunks.at(point) {
                Some(chunk) => chunk.is_passable(point),
                None => self.ground_is_passable(point),
            },
        }
    }

    /// Whether sight and light cross `point`: [`Map::sight`] on the site,
    /// the ground off it — which is all of it, the wasteland having no
    /// walls. Off a map with no wasteland, opaque.
    #[inline]
    pub fn is_see_through(&self, point: Point) -> bool {
        match self.size.index_of(point) {
            Some(index) => self.sight.is_passable_at(index),
            None => match self.chunks.at(point) {
                Some(chunk) => chunk.is_see_through(point),
                None => self.ground(point).is_some_and(TerrainId::is_see_through),
            },
        }
    }

    /// What a route pays to step into `point` when it is [passable]: the
    /// terrain's [`Terrain::cost`] — a pavement 1, a road 5. A prop changes
    /// whether a cell can be walked, never what walking it costs, so this is
    /// read straight off the terrain rather than kept as a third derived
    /// layer that edits would have to keep in step. Zero off the map and on
    /// anything that cannot be walked; ask [`Map::is_passable`] first.
    ///
    /// [passable]: Map::is_passable
    #[inline]
    pub fn walk_cost(&self, point: Point) -> u8 {
        self.terrain(point).map_or(0, TerrainId::walk_cost)
    }

    pub fn objects(&self, layer: ObjectLayer) -> &[Object] {
        &self.objects[layer as usize]
    }

    /// Objects are not bounds-checked against the terrain.
    ///
    /// They are free placement, and the useful ones sit exactly where the grid
    /// ends: a wall lamp on the last row, a crate half a cell past the last
    /// floor tile. Clamping them to the terrain rectangle would move somebody's
    /// prop rather than protect anything. A blocking prop off the map blocks
    /// nothing, there being no cell there to block.
    pub fn add_object(&mut self, layer: ObjectLayer, object: Object) {
        if layer == ObjectLayer::Props && !object.kind.prop_passability().is_passable() {
            self.invalidate_navigation();
            self.passability.set(object.cell(), false);
        }
        self.objects[layer as usize].push(object);
    }

    /// Drop the first object equal to this one; returns whether one went.
    ///
    /// One, not all: the editor deletes one sprite per click, and props stack
    /// deliberately, so removing every duplicate would leave entities on
    /// screen with nothing behind them.
    pub fn remove_object(&mut self, layer: ObjectLayer, object: &Object) -> bool {
        let objects = &mut self.objects[layer as usize];
        match objects.iter().position(|placed| placed == object) {
            Some(index) => {
                objects.remove(index);
                if layer == ObjectLayer::Props {
                    // Another prop may still stand in that cell.
                    self.refresh_passability(object.cell());
                }
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> Map {
        // A 5x5 floor with a wall down the middle column.
        let mut map = Map::new(Size::new(5, 5), FLOOR);
        for y in 0..5 {
            map.set_terrain(Point::new(2, y), WALL);
        }
        map
    }

    #[test]
    fn a_new_map_defines_every_cell() {
        let map = Map::new(Size::new(6, 4), FLOOR);
        assert_eq!(map.terrain_layers(), 1);
        assert_eq!(map.base_layer().tiles().len(), map.size().area());
        assert!(map.size().points().all(|p| map.terrain(p) == Some(FLOOR)));
    }

    #[test]
    fn passability_follows_the_terrain_it_was_derived_from() {
        let map = room();
        for point in map.size().points() {
            assert_eq!(
                map.is_passable(point),
                map.terrain(point).unwrap().is_passable(),
                "{point:?}"
            );
        }
        assert_eq!(map.passability().count_passable(), 20);
    }

    #[test]
    fn painting_a_cell_updates_passability_immediately() {
        // The whole point of deriving passability is that it cannot drift out
        // of step with the terrain, so an edit has to move both.
        let mut map = room();
        let door = Point::new(2, 2);
        assert!(map.passability().is_blocked(door));

        assert!(map.set_terrain(door, FLOOR));
        assert!(map.is_passable(door));

        assert!(map.set_terrain(door, WALL));
        assert!(!map.is_passable(door));
    }

    #[test]
    fn a_rebuild_agrees_with_the_incremental_updates() {
        let mut map = room();
        map.set_terrain(Point::new(2, 2), FLOOR);
        let incremental: Vec<bool> = map.size().points().map(|p| map.is_passable(p)).collect();

        map.rebuild_passability();
        let rebuilt: Vec<bool> = map.size().points().map(|p| map.is_passable(p)).collect();

        assert_eq!(incremental, rebuilt);
    }

    #[test]
    fn edits_off_the_map_are_refused_rather_than_wrapped() {
        let mut map = Map::new(Size::new(3, 3), FLOOR);
        let outside = Point::new(-1, 0);
        assert!(!map.set_terrain(outside, WALL));
        assert_eq!(map.terrain(outside), None);
        assert!(!map.is_passable(outside));
        // Nothing on the map moved.
        assert_eq!(map.passability().count_passable(), 9);
    }

    #[test]
    fn an_unbuilt_map_is_solid_void() {
        let map = Map::empty(Size::new(4, 4));
        assert!(map.size().points().all(|p| map.terrain(p) == Some(VOID)));
        assert_eq!(map.passability().count_passable(), 0);
    }

    fn bed(at: Point) -> Object {
        Object {
            at,
            kind: ObjectKind::new("bed 1"),
        }
    }

    #[test]
    fn object_layers_are_separate_lists() {
        let mut map = Map::new(Size::new(4, 4), FLOOR);
        let at = Point::new(64, 32);
        map.add_object(ObjectLayer::Props, bed(at));
        map.add_object(ObjectLayer::Spawners, bed(at));

        assert_eq!(map.objects(ObjectLayer::Props).len(), 1);
        assert!(map.remove_object(ObjectLayer::Props, &bed(at)));
        assert!(map.objects(ObjectLayer::Props).is_empty());
        // The other layer kept its own copy.
        assert_eq!(map.objects(ObjectLayer::Spawners).len(), 1);
    }

    #[test]
    fn erasing_a_stack_of_props_takes_one_at_a_time() {
        let mut map = Map::new(Size::new(4, 4), FLOOR);
        let at = Point::new(10, 10);
        map.add_object(ObjectLayer::Props, bed(at));
        map.add_object(ObjectLayer::Props, bed(at));

        assert!(map.remove_object(ObjectLayer::Props, &bed(at)));
        assert_eq!(map.objects(ObjectLayer::Props).len(), 1);
        assert!(map.remove_object(ObjectLayer::Props, &bed(at)));
        assert!(!map.remove_object(ObjectLayer::Props, &bed(at)));
    }

    #[test]
    fn an_object_is_in_the_cell_its_pixel_position_falls_in() {
        let at = |x, y| bed(Point::new(x, y)).cell();
        assert_eq!(at(0, 0), Point::new(0, 0));
        assert_eq!(at(PIXELS_PER_CELL - 1, PIXELS_PER_CELL), Point::new(0, 1));
        assert_eq!(at(PIXELS_PER_CELL * 3 + 24, 24), Point::new(3, 0));
        assert_eq!(at(-1, -PIXELS_PER_CELL), Point::new(-1, -1));
    }

    fn prop(name: &str, cell: Point) -> Object {
        Object {
            at: Point::new(cell.x * PIXELS_PER_CELL + 24, cell.y * PIXELS_PER_CELL + 24),
            kind: ObjectKind::new(name),
        }
    }

    #[test]
    fn a_prop_blocks_the_cell_its_centre_is_in_and_no_other() {
        let mut map = Map::new(Size::new(5, 5), FLOOR);
        map.add_object(ObjectLayer::Props, prop("fridge", Point::new(2, 3)));

        assert!(!map.is_passable(Point::new(2, 3)));
        assert_eq!(map.passability().count_passable(), 24);
    }

    #[test]
    fn a_prop_on_a_cell_boundary_blocks_the_cell_its_centre_landed_in() {
        let mut map = Map::new(Size::new(3, 3), FLOOR);
        let on_the_line = Object {
            at: Point::new(PIXELS_PER_CELL, PIXELS_PER_CELL),
            kind: ObjectKind::new("crate"),
        };
        map.add_object(ObjectLayer::Props, on_the_line);
        assert!(!map.is_passable(Point::new(1, 1)));
        assert_eq!(map.passability().count_passable(), 8);
    }

    #[test]
    fn a_spawner_blocks_nothing() {
        let mut map = Map::new(Size::new(3, 3), FLOOR);
        map.add_object(ObjectLayer::Spawners, prop("fridge", Point::new(1, 1)));
        assert_eq!(map.passability().count_passable(), 9);
    }

    #[test]
    fn painting_floor_under_a_prop_leaves_its_cell_blocked() {
        let mut map = Map::new(Size::new(3, 3), WALL);
        let cell = Point::new(1, 1);
        map.add_object(ObjectLayer::Props, prop("bed 1", cell));

        assert!(map.set_terrain(cell, FLOOR));
        assert!(!map.is_passable(cell));
    }

    #[test]
    fn a_cell_opens_only_when_the_last_prop_in_it_goes() {
        let mut map = Map::new(Size::new(3, 3), FLOOR);
        let cell = Point::new(1, 1);
        map.add_object(ObjectLayer::Props, prop("crate", cell));
        map.add_object(ObjectLayer::Props, prop("fridge", cell));

        assert!(map.remove_object(ObjectLayer::Props, &prop("crate", cell)));
        assert!(!map.is_passable(cell), "the fridge is still there");
        assert!(map.remove_object(ObjectLayer::Props, &prop("fridge", cell)));
        assert!(map.is_passable(cell));
    }

    #[test]
    fn taking_a_prop_off_a_wall_leaves_a_wall() {
        let mut map = Map::new(Size::new(3, 3), WALL);
        let cell = Point::new(1, 1);
        map.add_object(ObjectLayer::Props, prop("crate", cell));
        assert!(map.remove_object(ObjectLayer::Props, &prop("crate", cell)));
        assert!(!map.is_passable(cell));
    }

    #[test]
    fn a_rebuild_agrees_with_the_incremental_updates_props_included() {
        let mut map = room();
        map.add_object(ObjectLayer::Props, prop("fridge", Point::new(0, 0)));
        map.add_object(ObjectLayer::Props, prop("toilet", Point::new(4, 4)));
        map.add_object(ObjectLayer::Props, prop("crate", Point::new(3, 1)));
        map.remove_object(ObjectLayer::Props, &prop("crate", Point::new(3, 1)));
        map.set_terrain(Point::new(0, 0), FLOOR);
        let incremental: Vec<bool> = map.size().points().map(|p| map.is_passable(p)).collect();

        map.rebuild_passability();
        let rebuilt: Vec<bool> = map.size().points().map(|p| map.is_passable(p)).collect();

        assert_eq!(incremental, rebuilt);
        assert!(!map.is_passable(Point::new(0, 0)) && !map.is_passable(Point::new(4, 4)));
        assert!(map.is_passable(Point::new(3, 1)));
    }

    #[test]
    fn a_new_map_is_open_sky_and_a_ceiling_blocks_nobody() {
        let mut map = Map::new(Size::new(3, 3), FLOOR);
        assert_eq!(map.ceiling_count(), 0);
        assert!(map.set_ceiling(Point::new(1, 1), true));
        assert!(map.has_ceiling(Point::new(1, 1)));
        assert!(!map.has_ceiling(Point::new(0, 1)));
        assert!(map.is_passable(Point::new(1, 1)), "a roof is not in anybody's way");
        assert!(!map.set_ceiling(Point::new(5, 5), true), "off the map is refused");
        assert!(!map.has_ceiling(Point::new(-1, 0)), "off the map is open sky");
    }

    #[test]
    fn a_lamp_blocks_nothing() {
        let mut map = Map::new(Size::new(3, 3), FLOOR);
        map.add_object(ObjectLayer::Lamps, prop("ceiling lamp", Point::new(1, 1)));
        assert_eq!(map.passability().count_passable(), 9);
    }

    #[test]
    fn an_object_may_sit_outside_the_terrain() {
        // Props are not cells: one hanging off the edge is a placement, not a
        // corruption.
        let mut map = Map::new(Size::new(2, 2), FLOOR);
        let outside = Point::new(-30, 400);
        map.add_object(ObjectLayer::Props, bed(outside));
        assert_eq!(map.objects(ObjectLayer::Props), [bed(outside)]);
    }

    /// A small site in the wasteland of seed 3, and the first heap of scrap
    /// out past its clearing holding `pieces` pieces.
    fn scrapyard(pieces: u8) -> (Map, Point) {
        let map = Map::new(Size::new(8, 8), FLOOR).with_wasteland(3);
        let mut found = None;
        map.heaps(Bounds::new(Point::new(40, -200), Size::new(400, 400)), |cell, heap| {
            if found.is_none() && heap.pieces == pieces {
                found = Some(cell);
            }
        });
        (map, found.expect("a heap in a patch that size"))
    }

    #[test]
    fn a_heap_of_scrap_is_in_the_way_whether_its_chunk_is_loaded_or_not() {
        let (mut map, heap) = scrapyard(2);
        assert_eq!(map.scrap(heap).map(|h| h.pieces), Some(2));
        assert!(!map.is_passable(heap), "unloaded");
        assert!(map.is_see_through(heap), "a heap is seen over");
        assert!(map.is_open_ground(heap), "and could be cleared away");
        map.load_chunk(ChunkPos::of(heap));
        assert!(!map.is_passable(heap), "loaded");
        assert_eq!(map.scrap(heap).map(|h| h.pieces), Some(2));
        let beside = heap.cardinal_neighbours();
        assert!(beside.iter().any(|&cell| map.is_passable(cell)), "something to work it from");
    }

    #[test]
    fn taking_the_last_piece_clears_a_heap_away_and_opens_its_cell() {
        for loaded in [false, true] {
            let (mut map, heap) = scrapyard(2);
            if loaded {
                map.load_chunk(ChunkPos::of(heap));
            }
            let revision = map.salvage_revision();
            assert_eq!(map.take_scrap(heap), Some(1), "loaded {loaded}");
            assert!(map.salvage_revision() > revision);
            assert_eq!(map.scrap(heap).map(|h| h.pieces), Some(1));
            assert!(!map.is_passable(heap), "one piece is still a heap");
            assert_eq!(map.take_scrap(heap), Some(0));
            assert_eq!(map.scrap(heap), None);
            assert!(map.is_passable(heap), "loaded {loaded}: cleared away");
            assert_eq!(map.take_scrap(heap), None, "nothing left to take");
            let mut listed = false;
            map.heaps(Bounds::new(heap, Size::new(1, 1)), |_, _| listed = true);
            assert!(!listed, "an empty heap is not drawn");
        }
    }

    #[test]
    fn a_heap_emptied_stays_empty_when_its_chunk_is_dropped_and_loaded_again() {
        let (mut map, heap) = scrapyard(1);
        let pos = ChunkPos::of(heap);
        map.load_chunk(pos);
        assert_eq!(map.take_scrap(heap), Some(0));
        map.unload_chunks(|_| false);
        assert!(map.is_passable(heap), "dropped");
        map.load_chunk(pos);
        assert!(map.is_passable(heap), "loaded again");
        assert_eq!(map.scrap(heap), None);
    }

    #[test]
    fn heaps_read_the_same_from_a_loaded_chunk_as_from_the_generator() {
        let (mut map, heap) = scrapyard(3);
        let area = Bounds::new(heap.offset(-40, -40), Size::new(80, 80));
        let gather = |map: &Map| {
            let mut heaps = Vec::new();
            map.heaps(area, |cell, scrap| heaps.push((cell, scrap)));
            heaps.sort_by_key(|&(cell, _)| cell);
            heaps
        };
        let unloaded = gather(&map);
        assert!(unloaded.len() > 1);
        map.load_chunks(area);
        assert_eq!(gather(&map), unloaded);
        for cell in area.points() {
            assert_eq!(map.is_passable(cell), map.terrain(cell).is_some_and(TerrainId::is_passable) && map.scrap(cell).is_none());
        }
    }

    #[test]
    fn a_new_wasteland_forgets_what_was_salvaged_from_the_old_one() {
        let (mut map, heap) = scrapyard(1);
        assert_eq!(map.take_scrap(heap), Some(0));
        map.set_wasteland(None);
        map.set_wasteland(Some(Wasteland::new(3)));
        assert_eq!(map.scrap(heap).map(|h| h.pieces), Some(1));
    }

    #[test]
    fn there_is_no_scrap_on_the_site_or_round_a_map_with_nothing_round_it() {
        let (map, heap) = scrapyard(1);
        assert!((0..8).all(|x| (0..8).all(|y| map.scrap(Point::new(x, y)).is_none())));
        let bare = Map::new(Size::new(8, 8), FLOOR);
        assert_eq!(bare.scrap(heap), None);
    }

    /// A small site in the wasteland of seed 3, and the first plot of meadow
    /// out past it.
    fn meadow() -> (Map, Point) {
        let map = Map::new(Size::new(8, 8), FLOOR).with_wasteland(3);
        let plot = Bounds::new(Point::new(40, -200), Size::new(400, 400))
            .points()
            .find(|&cell| map.can_sow(cell))
            .expect("a plot of meadow in a patch that size");
        (map, plot)
    }

    #[test]
    fn a_crop_is_sown_only_in_a_plot_of_good_soil() {
        let (map, plot) = meadow();
        assert_eq!(map.terrain(plot), Some(MEADOW));
        assert!(is_plot(plot) && !is_plot(plot.offset(1, 0)) && !is_plot(plot.offset(0, 1)) && !is_plot(plot.offset(1, 1)));
        assert!(!map.can_sow(plot.offset(1, 0)), "not a plot");
        let mut floor = Map::new(Size::new(8, 8), FLOOR);
        assert!(!floor.can_sow(Point::new(2, 2)), "a floor is not soil");
        assert!(!floor.plant(Point::new(2, 2)));
        assert!(floor.is_passable(Point::new(2, 2)));
        let mut on_site = Map::new(Size::new(8, 8), MEADOW);
        assert!(on_site.plant(Point::new(4, 2)), "meadow painted on the site is soil too");
        assert!(!on_site.is_passable(Point::new(4, 2)));
        assert!(!on_site.plant(Point::new(4, 2)), "one crop to a plot");
        assert!(on_site.unplant(Point::new(4, 2)));
        assert!(on_site.is_passable(Point::new(4, 2)));
        assert!(!on_site.unplant(Point::new(4, 2)), "nothing left to harvest");
    }

    /// A wall across a meadow with one gap in it, and the gap a plot: sown,
    /// it would cut the meadow in two, so it cannot be — nor anything with a
    /// blocked cell round it — while a plot out in the open can.
    #[test]
    fn a_plot_is_sown_only_with_the_ground_open_all_round_it() {
        let mut map = Map::new(Size::new(10, 10), MEADOW);
        for y in (0..10).filter(|&y| y != 4) {
            map.set_terrain(Point::new(4, y), WALL);
        }
        assert!(!map.can_sow(Point::new(4, 4)), "the one way through");
        assert!(map.can_sow(Point::new(2, 2)) && map.can_sow(Point::new(6, 6)), "open all round");
        assert!(!map.can_sow(Point::new(0, 2)), "the map's edge, with nothing past it");
        assert!(!map.plant(Point::new(4, 4)));
        assert!(map.is_passable(Point::new(4, 4)));
    }

    /// Sown, a plot is in the way as a heap is, whether its chunk is loaded
    /// or not, dropped and loaded again or loaded only after — and
    /// reachability never hears of it.
    #[test]
    fn a_crop_is_in_the_way_until_it_is_harvested_whether_its_chunk_is_loaded_or_not() {
        for loaded in [false, true] {
            let (mut map, plot) = meadow();
            let pos = ChunkPos::of(plot);
            if loaded {
                map.load_chunk(pos);
            }
            let navigation = map.navigation_revision();
            assert!(map.plant(plot), "loaded {loaded}");
            assert!(map.is_planted(plot) && !map.is_passable(plot), "loaded {loaded}");
            assert!(map.is_open_ground(plot) && map.is_see_through(plot), "loaded {loaded}");
            assert!(plot.cardinal_neighbours().iter().all(|&side| !map.is_planted(side)));
            map.unload_chunks(|_| false);
            assert!(!map.is_passable(plot), "dropped");
            map.load_chunk(pos);
            assert!(!map.is_passable(plot), "loaded again");
            assert_eq!(map.planted().collect::<Vec<_>>(), [plot]);
            assert!(map.unplant(plot));
            assert!(map.is_passable(plot), "harvested");
            map.unload_chunks(|_| false);
            assert!(map.is_passable(plot), "harvested and dropped");
            assert_eq!(map.navigation_revision(), navigation, "a field changes nothing about who can reach what");
        }
    }

    #[test]
    fn a_new_wasteland_forgets_what_was_sown_in_the_old_one() {
        let (mut map, plot) = meadow();
        assert!(map.plant(plot));
        map.set_wasteland(None);
        map.set_wasteland(Some(Wasteland::new(3)));
        assert!(!map.is_planted(plot) && map.is_passable(plot));
    }
}
