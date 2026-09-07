//! Terrain tiles and what they mean to somebody trying to walk on them.
//!
//! A tile is an id into a static catalogue, not a struct stored per cell: the
//! map holds one [`TerrainId`] per cell per layer, so a big map is a flat
//! array of `u16` rather than an array of anything that has to be cloned.
//! Everything a tile *is* — for now only whether it can be walked on — lives
//! once in [`TERRAIN`].
//!
//! Which PNG draws a tile is deliberately not here. Art belongs to the Bevy
//! adapter, and keeping it out is what lets the simulation be tested without
//! an `App`, an `AssetServer` or the `assets/` directory. The link between the
//! two is the **name**: the editor's background palette carries the art and
//! names each entry after the tile it paints, and `editor::background` has a
//! test that the two lists still line up. A name is also what a map file
//! stores, so art and passability can be re-decided without rewriting maps.

/// Whether a cell can be walked through.
///
/// A two-state enum rather than a `bool` because the call sites read as
/// claims about the world (`terrain.passability`) instead of as a flag whose
/// polarity you have to go and look up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Passability {
    Passable,
    Impassable,
}

impl Passability {
    pub const fn is_passable(self) -> bool {
        matches!(self, Passability::Passable)
    }
}

/// One entry in the terrain catalogue.
pub struct Terrain {
    pub name: &'static str,
    pub passability: Passability,
}

impl Terrain {
    const fn walkable(name: &'static str) -> Self {
        Self {
            name,
            passability: Passability::Passable,
        }
    }

    const fn blocked(name: &'static str) -> Self {
        Self {
            name,
            passability: Passability::Impassable,
        }
    }
}

/// A cell's terrain: an index into [`TERRAIN`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TerrainId(pub u16);

/// Nothing has been built here.
///
/// The map format requires every cell of every terrain layer to be defined,
/// so "unpainted" has to be a tile like any other rather than an absent one.
/// Making it impassable means a half-authored map fences the crowd in instead
/// of leaking it into the void.
pub const VOID: TerrainId = TerrainId(0);
pub const FLOOR: TerrainId = TerrainId(1);
/// The plain brown wall — the default when something just needs to block.
pub const WALL: TerrainId = TerrainId(9);

/// Every tile that can be painted, in the order the editor palette offers
/// them. Names are what map files store, so renaming one invalidates saved
/// maps; adding to the end never does.
pub const TERRAIN: &[Terrain] = &[
    Terrain::blocked("void"),
    Terrain::walkable("floor"),
    Terrain::walkable("floor white"),
    Terrain::walkable("floor red"),
    Terrain::walkable("floor colorful"),
    Terrain::walkable("floor diagonal"),
    Terrain::walkable("tiles blue"),
    Terrain::walkable("tiles yellow"),
    Terrain::walkable("wood"),
    Terrain::blocked("wall brown"),
    Terrain::blocked("wall brown big"),
    Terrain::blocked("wall purple"),
    Terrain::blocked("wall red"),
    Terrain::blocked("block"),
    Terrain::walkable("wood cracked"),
];

impl TerrainId {
    /// The catalogue entry, or `None` for an id this build does not know.
    ///
    /// An unknown id is reachable: a map file written by a later version, or
    /// by an editor with a bigger palette, can name a tile that is not in
    /// this binary. That is a load-time complaint, not a reason to panic in
    /// the middle of a tick.
    pub fn terrain(self) -> Option<&'static Terrain> {
        TERRAIN.get(self.0 as usize)
    }

    pub fn name(self) -> &'static str {
        self.terrain().map_or("unknown", |terrain| terrain.name)
    }

    /// The tile with this name, or `None` if this build has no such tile.
    ///
    /// Map files store names rather than ids, so this is how they are read
    /// back: an id is an index into a table that a later version is free to
    /// reorder, but a name is the thing the author actually meant.
    pub fn from_name(name: &str) -> Option<TerrainId> {
        TERRAIN
            .iter()
            .position(|terrain| terrain.name == name)
            .map(|index| TerrainId(index as u16))
    }

    /// Unknown terrain is impassable: an agent that cannot be told what a tile
    /// is should walk around it, not through it.
    pub fn is_passable(self) -> bool {
        self.terrain()
            .is_some_and(|terrain| terrain.passability.is_passable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The named constants are raw indices into a hand-written table, so
    /// inserting an entry in the middle would silently rename every tile in
    /// every saved map.
    #[test]
    fn the_named_tiles_are_the_catalogue_entries_they_claim_to_be() {
        assert_eq!(VOID.name(), "void");
        assert_eq!(FLOOR.name(), "floor");
        assert_eq!(WALL.name(), "wall brown");
    }

    #[test]
    fn the_named_tiles_can_be_walked_on_or_not_as_expected() {
        assert!(FLOOR.is_passable());
        assert!(!WALL.is_passable());
        assert!(!VOID.is_passable());
    }

    /// Names are the key a map file stores a tile under, so two tiles sharing
    /// one would make a saved map ambiguous.
    #[test]
    fn every_tile_has_its_own_name() {
        let mut names: Vec<&str> = TERRAIN.iter().map(|terrain| terrain.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate terrain name");
    }

    #[test]
    fn a_tile_can_be_found_again_by_the_name_it_is_stored_under() {
        for (index, terrain) in TERRAIN.iter().enumerate() {
            assert_eq!(TerrainId::from_name(terrain.name), Some(TerrainId(index as u16)));
        }
        assert_eq!(TerrainId::from_name("linoleum"), None);
    }

    #[test]
    fn a_tile_this_build_does_not_know_is_impassable() {
        let from_a_newer_map = TerrainId(u16::MAX);
        assert_eq!(from_a_newer_map.terrain().map(|t| t.name), None);
        assert!(!from_a_newer_map.is_passable());
    }
}
