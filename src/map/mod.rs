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
//!   [`ObjectLayer`]: props and spawners. Both are placeholders. Props are not
//!   read yet (the editor still spawns its own entities), and what a spawner
//!   is has deliberately not been decided.
//!
//! Maps serialise to JSON — see [`format`] for the on-disk shape, and
//! [`Map::to_json`] / [`Map::from_json`].
//!
//! Passability is *derived*, not authored: it is rebuilt from the terrain and
//! kept in sync by [`Map::set_terrain`], so a map cannot end up claiming a
//! wall can be walked through. See [`PassabilityMap`] for why it is a separate
//! structure at all.

// The map is the foundation for a simulation that does not exist yet, so most
// of it is reached only from tests until there is something to walk around on
// it. In a binary crate that makes the public surface, re-exports included,
// look unused; the alternative is trimming the API down to whatever today's
// callers happen to touch.
#![allow(dead_code, unused_imports)]

mod coords;
mod format;
mod passability;
mod storage;
mod terrain;

pub use coords::{Point, Size};
pub use format::{MapFormatError, FORMAT_VERSION};
pub use passability::PassabilityMap;
pub use storage::{sanitize_name, MapStore, StorageError, MAX_NAME};
pub use terrain::{Passability, Terrain, TerrainId, FLOOR, TERRAIN, VOID, WALL};

use serde::{Deserialize, Serialize};

/// The terrain layer every map has. Overlay layers, when they arrive, are
/// indices above this one.
pub const BASE: usize = 0;

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
}

impl ObjectLayer {
    pub const ALL: [ObjectLayer; 2] = [ObjectLayer::Props, ObjectLayer::Spawners];

    /// The key this layer is stored under in a map file. Renaming one is a
    /// format change, not a cosmetic edit.
    pub const fn name(self) -> &'static str {
        match self {
            ObjectLayer::Props => "props",
            ObjectLayer::Spawners => "spawners",
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
/// Spawners are still a placeholder — nothing writes to that layer yet.
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

/// Deliberately summarised rather than derived: a derived `Debug` on a map
/// prints every cell, which in a test failure buries the assertion that
/// actually matters under a few thousand tile ids.
impl std::fmt::Debug for Map {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Map")
            .field("size", &self.size)
            .field("terrain_layers", &self.terrain.len())
            .field("props", &self.objects(ObjectLayer::Props).len())
            .field("spawners", &self.objects(ObjectLayer::Spawners).len())
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
    passability: PassabilityMap,
}

impl Map {
    /// A map of `size` with its base terrain layer filled with `tile`.
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
            passability: PassabilityMap::new(size),
        };
        map.rebuild_passability();
        map
    }

    /// An unbuilt map: every cell [`VOID`], and so nothing walkable anywhere.
    pub fn empty(size: Size) -> Self {
        Self::new(size, VOID)
    }

    pub fn size(&self) -> Size {
        self.size
    }

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

    /// The base-layer tile at `point`, or `None` off the map.
    pub fn terrain(&self, point: Point) -> Option<TerrainId> {
        self.size
            .index_of(point)
            .map(|index| self.terrain[BASE].tiles[index])
    }

    /// Paint a base-layer cell, keeping the passability map in step. Returns
    /// whether the point was on the map.
    pub fn set_terrain(&mut self, point: Point, tile: TerrainId) -> bool {
        let Some(index) = self.size.index_of(point) else {
            return false;
        };
        self.terrain[BASE].tiles[index] = tile;
        self.passability.set(point, tile.is_passable());
        true
    }

    /// Recompute passability for the whole map.
    ///
    /// Only needed after a bulk change that bypassed [`Map::set_terrain`] —
    /// loading a file, say. It is also the one place the rule lives, so when
    /// overlay layers arrive (a rug over a floor, a table over both) combining
    /// them happens here and every caller keeps asking the same question.
    pub fn rebuild_passability(&mut self) {
        for (index, tile) in self.terrain[BASE].tiles.iter().enumerate() {
            self.passability
                .set(self.size.point_at(index), tile.is_passable());
        }
    }

    /// The static passability map, for pathfinding and steering to hold on to.
    pub fn passability(&self) -> &PassabilityMap {
        &self.passability
    }

    /// Whether the terrain lets an agent stand at `point`. Off the map is not
    /// passable.
    pub fn is_passable(&self, point: Point) -> bool {
        self.passability.is_passable(point)
    }

    pub fn objects(&self, layer: ObjectLayer) -> &[Object] {
        &self.objects[layer as usize]
    }

    /// Objects are not bounds-checked against the terrain.
    ///
    /// They are free placement, and the useful ones sit exactly where the grid
    /// ends: a wall lamp on the last row, a crate half a cell past the last
    /// floor tile. Clamping them to the terrain rectangle would move somebody's
    /// prop rather than protect anything — nothing indexes an object by cell.
    pub fn add_object(&mut self, layer: ObjectLayer, object: Object) {
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
    fn an_object_may_sit_outside_the_terrain() {
        // Props are not cells: one hanging off the edge is a placement, not a
        // corruption.
        let mut map = Map::new(Size::new(2, 2), FLOOR);
        let outside = Point::new(-30, 400);
        map.add_object(ObjectLayer::Props, bed(outside));
        assert_eq!(map.objects(ObjectLayer::Props), [bed(outside)]);
    }
}
