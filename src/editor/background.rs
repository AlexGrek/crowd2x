//! The background layer: floors and walls, snapped to a grid.
//!
//! A background tile is the simpler of the two sprite kinds. It is locked to a
//! cell, there is at most one per cell, and every tile sits at the same depth
//! far behind the world — the background takes no part in the painter's-order
//! sort that characters and props use, so it can never end up in front of an
//! actor no matter where it is painted.
//!
//! This layer is the drawn face of [`Map`]'s terrain: painting writes the tile
//! into the map first and only spawns a sprite if the map accepted it, so the
//! screen cannot show a floor that no saved map contains. The two are linked
//! by **name** — a palette entry is called after the terrain it paints — which
//! keeps the art here and the passability in `map::terrain` without either
//! needing an index into the other.
//!
//! Contrast [`super::props`], where placement is free and depth comes from
//! world Y.

use std::collections::HashMap;

use bevy::prelude::*;

use super::PaletteItem;
use crate::characters::{upscale, CELL};
use crate::map::{Map, Point, TerrainId, VOID};
use crate::render::WORLD_LAYER;
use crate::state::AppState;

/// Tiles are laid out on the character cell grid. The art is drawn at 16x16 and
/// upscaled to fill it — each palette entry carries the scale that gets it
/// there, since a few imported tiles are still stored at the full cell size.
pub const TILE: f32 = CELL as f32;

/// Depth of every background tile. `characters::depth_for` is `-y * 0.01`, so
/// it would take a character at y = 10000 to reach this — far outside anything
/// the editor can place.
const BACKGROUND_Z: f32 = -100.0;

/// Every paintable tile. Each name must be a terrain in `map::terrain`, which
/// `every_palette_entry_names_a_terrain` checks — a typo here would
/// otherwise be a tile that paints on screen and vanishes when saved.
pub const PALETTE: &[PaletteItem] = &[
    PaletteItem::new("floor", "floor.png"),
    PaletteItem::upscaled("floor white", "floor_white.png"),
    PaletteItem::upscaled("floor red", "floor_red.png"),
    PaletteItem::upscaled("floor colorful", "floor_colorful.png"),
    PaletteItem::new("floor diagonal", "floor_diag.png"),
    PaletteItem::upscaled("tiles blue", "floor_tiles_blue.png"),
    PaletteItem::upscaled("tiles yellow", "floor_tiles_yellow.png"),
    PaletteItem::upscaled("wood", "wood.png"),
    PaletteItem::upscaled("wood cracked", "wood_crack.png"),
    PaletteItem::upscaled("wall brown", "wall_brown.png"),
    PaletteItem::upscaled("wall brown big", "wall_brown_big.png"),
    PaletteItem::upscaled("wall purple", "wall_purple.png"),
    PaletteItem::upscaled("wall red", "wall_red.png"),
    PaletteItem::upscaled("block", "block.png"),
];

#[derive(Component)]
pub struct BackgroundTile;

/// What is currently drawn, so a cell can be replaced or erased without
/// scanning every tile in the map.
#[derive(Resource, Default)]
pub struct Tiles(HashMap<IVec2, Painted>);

impl Tiles {
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

struct Painted {
    entity: Entity,
    /// Index into [`PALETTE`], so re-painting a cell with what is already
    /// there can be skipped.
    item: usize,
}

/// The cell containing a world position.
pub fn cell_of(world: Vec2) -> IVec2 {
    (world / TILE).floor().as_ivec2()
}

/// Centre of a cell in world space — where its sprite sits.
pub fn cell_centre(cell: IVec2) -> Vec2 {
    cell.as_vec2() * TILE + Vec2::splat(TILE / 2.0)
}

/// How much world a map covers, in pixels. Cell (0, 0) starts at the origin,
/// so this is also the map's far corner — which is what a camera has to be
/// kept inside of.
pub fn map_extent(map: &Map) -> Vec2 {
    let size = map.size();
    Vec2::new(size.width as f32, size.height as f32) * TILE
}

/// A cell as the map addresses it. The two grids are the same grid; only the
/// types differ, because one side of the fence has no `bevy` in it.
pub fn point_of(cell: IVec2) -> Point {
    Point::new(cell.x, cell.y)
}

/// The terrain a palette entry paints.
pub fn terrain_of(item: usize) -> Option<TerrainId> {
    TerrainId::from_name(PALETTE[item].name)
}

/// The palette entry that draws a terrain, or `None` for one this build has no
/// art for — a map from a version with a bigger palette.
pub fn item_of(terrain: TerrainId) -> Option<usize> {
    PALETTE.iter().position(|item| item.name == terrain.name())
}

/// Put `item` in `cell`, replacing whatever was there.
///
/// This is called every frame while the mouse is held, so re-painting a cell
/// with the tile it already has has to be free. Cells outside the map are
/// simply not painted: the map has a size, and silently growing it under the
/// cursor would make that size a lie.
pub fn paint(
    commands: &mut Commands,
    assets: &AssetServer,
    tiles: &mut Tiles,
    map: &mut Map,
    cell: IVec2,
    item: usize,
) {
    if tiles.0.get(&cell).is_some_and(|painted| painted.item == item) {
        return;
    }
    let Some(terrain) = terrain_of(item) else {
        return;
    };
    if !map.set_terrain(point_of(cell), terrain) {
        return;
    }

    if let Some(painted) = tiles.0.remove(&cell) {
        commands.entity(painted.entity).despawn();
    }
    let entity = spawn_tile(commands, assets, cell, item, AppState::Editor);
    tiles.0.insert(cell, Painted { entity, item });
}

pub fn erase(commands: &mut Commands, tiles: &mut Tiles, map: &mut Map, cell: IVec2) {
    if !map.set_terrain(point_of(cell), VOID) {
        return;
    }
    if let Some(painted) = tiles.0.remove(&cell) {
        commands.entity(painted.entity).despawn();
    }
}

/// Draw a whole map's terrain, for entering a screen with a map already
/// loaded. Void cells get no sprite at all — that is what makes them read as
/// nothing rather than as a black tile.
///
/// `state` is the screen the sprites belong to, because both the editor and
/// the game draw the same map and each has to take its own copy away with it.
/// The index is only wanted by the editor: a screen that cannot paint has no
/// use for knowing which entity is in which cell.
pub fn spawn_map(
    commands: &mut Commands,
    assets: &AssetServer,
    map: &Map,
    state: AppState,
    mut tiles: Option<&mut Tiles>,
) {
    if let Some(tiles) = tiles.as_deref_mut() {
        tiles.clear();
    }
    let mut unknown = Vec::new();

    for point in map.size().points() {
        let terrain = map.terrain(point).expect("point came from the map");
        if terrain == VOID {
            continue;
        }
        let Some(item) = item_of(terrain) else {
            if !unknown.contains(&terrain) {
                unknown.push(terrain);
            }
            continue;
        };
        let cell = IVec2::new(point.x, point.y);
        let entity = spawn_tile(commands, assets, cell, item, state);
        if let Some(tiles) = tiles.as_deref_mut() {
            tiles.0.insert(cell, Painted { entity, item });
        }
    }

    for terrain in unknown {
        // Kept in the map rather than dropped, so saving does not quietly
        // delete terrain this build merely cannot draw.
        warn!("no art for terrain {:?}, not drawn", terrain.name());
    }
}

fn spawn_tile(
    commands: &mut Commands,
    assets: &AssetServer,
    cell: IVec2,
    item: usize,
    state: AppState,
) -> Entity {
    let centre = cell_centre(cell);
    commands
        .spawn((
            Name::new("tile"),
            BackgroundTile,
            Sprite::from_image(assets.load(PALETTE[item].path)),
            Transform::from_xyz(centre.x, centre.y, BACKGROUND_Z)
                .with_scale(upscale(PALETTE[item].scale)),
            WORLD_LAYER,
            DespawnOnExit(state),
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR};

    #[test]
    fn a_cell_covers_tile_units_from_its_origin() {
        assert_eq!(cell_of(Vec2::ZERO), IVec2::ZERO);
        assert_eq!(cell_of(Vec2::splat(TILE - 0.1)), IVec2::ZERO);
        assert_eq!(cell_of(Vec2::splat(TILE)), IVec2::ONE);
    }

    #[test]
    fn cells_left_of_the_origin_round_down() {
        // Truncating instead of flooring would collapse the whole strip
        // between -TILE and 0 onto cell 0 and paint the wrong side of the axis.
        assert_eq!(cell_of(Vec2::splat(-0.1)), IVec2::splat(-1));
        assert_eq!(cell_of(Vec2::splat(-TILE)), IVec2::splat(-1));
        assert_eq!(cell_of(Vec2::splat(-TILE - 0.1)), IVec2::splat(-2));
    }

    #[test]
    fn a_cell_centre_is_inside_its_own_cell() {
        for cell in [IVec2::ZERO, IVec2::new(3, -7), IVec2::new(-4, -1)] {
            assert_eq!(cell_of(cell_centre(cell)), cell);
        }
    }

    /// The palette and the terrain catalogue are joined by name, and nothing
    /// at runtime re-checks it: a palette entry naming a tile that does not
    /// exist would paint on screen and be missing from the saved map.
    #[test]
    fn every_palette_entry_names_a_terrain() {
        for (index, item) in PALETTE.iter().enumerate() {
            let terrain = terrain_of(index)
                .unwrap_or_else(|| panic!("no terrain called {:?}", item.name));
            assert_eq!(item_of(terrain), Some(index), "{}", item.name);
        }
    }

    /// ...and the other way: a tile that can be saved but not drawn would come
    /// back from a file as a hole in the map.
    #[test]
    fn every_terrain_except_void_can_be_painted() {
        for (index, terrain) in crate::map::TERRAIN.iter().enumerate() {
            let id = TerrainId(index as u16);
            if id == VOID {
                continue;
            }
            assert!(item_of(id).is_some(), "no palette entry paints {:?}", terrain.name);
        }
    }

    #[test]
    fn the_editor_grid_and_the_map_grid_are_the_same_grid() {
        let mut map = Map::new(Size::new(4, 4), VOID);
        let cell = IVec2::new(2, 3);
        assert!(map.set_terrain(point_of(cell), FLOOR));
        assert_eq!(map.terrain(Point::new(2, 3)), Some(FLOOR));
        // ...and the sprite for that cell lands inside it.
        assert_eq!(cell_of(cell_centre(cell)), cell);
    }
}
