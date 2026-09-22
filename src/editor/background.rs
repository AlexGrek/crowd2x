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
//!
//! # A window, not a map
//!
//! Tiles are drawn **for the cells on the canvas and for no others**. A
//! 256x256 map is 65,536 cells; the canvas at the default zoom shows about
//! seven by four of them, so spawning a sprite per cell meant sixty-five
//! thousand entities to draw eighty tiles, paid for on entry as one enormous
//! `OnEnter` and then again on every frame, since Bevy walks every sprite
//! entity in the world before it checks whether any of them is visible.
//!
//! [`TileWindow`] is instead a small set of sprites that follow the camera:
//! when the view crosses a cell boundary, the column or row that left is
//! released and the one that arrived is drawn, re-pointing the same entities
//! rather than despawning and spawning them. The camera moves in sub-cell
//! steps, so on most frames the window has nothing at all to do.
//!
//! **The map stays the source of truth.** The window is a pure function of the
//! map and the view: painting writes the tile into the map and marks the
//! window out of date, and the window works out what that means for what is on
//! screen. That is the same invariant this module always had — the screen
//! cannot show a floor no saved map contains — arrived at from the other end,
//! and it is why painting no longer needs an index of its own.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use super::{CurrentMap, PaletteItem};
use crate::characters::upscale;
use crate::map::{Map, Point, TerrainId, VOID};
use crate::render::WORLD_LAYER;
use crate::state::AppState;
use crate::game::pool::park_budget;
use crate::view::{CellRect, VisibleArea};

/// Tiles are laid out on the character cell grid. The art is drawn at 16x16 and
/// upscaled to fill it — each palette entry carries the scale that gets it
/// there, since a few imported tiles are still stored at the full cell size.
///
/// Defined from [`crate::map::PIXELS_PER_CELL`], which is what a map file's
/// object positions are measured against, so the two cannot drift.
pub const TILE: f32 = crate::map::PIXELS_PER_CELL as f32;

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

/// The tiles on the canvas, and the sprites waiting to become one.
///
/// This is the tile index the editor always needed — painting has to find the
/// sprite already in a cell — and it is now also the whole of what is drawn.
/// Both screens use it: the editor and the game draw the same map, and one
/// windowed renderer for the terrain layer is better than two that can drift.
#[derive(Resource, Default)]
pub struct TileWindow {
    /// Cell to the sprite drawing it, and which palette entry that sprite is
    /// currently showing.
    drawn: HashMap<IVec2, Painted>,
    /// What `drawn` covers, so a camera that has not crossed a cell boundary
    /// does no work at all. Empty until the first fill.
    rect: CellRect,
    /// Sprites released by cells that left the view, hidden, waiting to be
    /// pointed at a cell that arrived.
    ///
    /// Bounded like the actor pool and for the same reason: a hidden sprite
    /// still costs a walk in every frame's extraction, so a free list that
    /// grew to the size of the map would give back what the window won.
    free: Vec<Entity>,
    /// Terrain this build has no art for, so it is warned about once per visit
    /// rather than once per frame — a per-frame `warn!` would put sixty lines
    /// a second into the log every QA test reads.
    unknown: HashSet<TerrainId>,
    /// Set when the map changed under the window, so the next run re-reads the
    /// cells it is showing instead of trusting what it drew last time.
    dirty: bool,
}

impl TileWindow {
    /// Forget everything without despawning it.
    ///
    /// The sprites carry `DespawnOnExit`, so leaving a screen already takes
    /// them; despawning here too would be a second command against an entity
    /// Bevy is in the middle of removing. Clearing on every state change is
    /// also what keeps the `DespawnOnExit` tag on a reused sprite correct —
    /// no sprite ever survives into the other screen to be re-pointed there.
    pub fn clear(&mut self) {
        self.drawn.clear();
        self.free.clear();
        self.rect = CellRect::EMPTY;
        self.unknown.clear();
        self.dirty = false;
    }

    /// Say the map changed, so the window re-reads what it is showing.
    ///
    /// Painting calls this *after* writing the map, never instead of it: the
    /// window redraws from the map, so a cell it has not been told about is a
    /// cell that still shows whatever the map says it shows.
    pub fn touch(&mut self) {
        self.dirty = true;
    }

}

struct Painted {
    entity: Entity,
    /// Index into [`PALETTE`], so a cell whose tile has not changed can be
    /// left alone.
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
///
/// **The map is written, and nothing else.** What is on screen follows from
/// the map by way of [`sync_tile_window`], so this cannot draw a tile the
/// saved file does not contain — the invariant this module was always built
/// on, now held by construction rather than by remembering to do the two in
/// the right order.
pub fn paint(window: &mut TileWindow, map: &mut Map, cell: IVec2, item: usize) {
    let Some(terrain) = terrain_of(item) else {
        return;
    };
    let at = point_of(cell);
    // Asked of the map rather than of an index kept beside it: a flat array
    // read, and one fewer thing that can disagree with the file.
    if map.terrain(at) == Some(terrain) {
        return;
    }
    if !map.set_terrain(at, terrain) {
        return;
    }
    window.touch();
}

pub fn erase(window: &mut TileWindow, map: &mut Map, cell: IVec2) {
    if !map.set_terrain(point_of(cell), VOID) {
        return;
    }
    window.touch();
}

/// The palette entry a cell should show, or `None` for one that draws nothing:
/// off the map, `VOID`, or terrain this build has no art for.
///
/// Void getting no sprite at all is what makes it read as nothing rather than
/// as a black tile.
fn wanted_item(map: &Map, cell: IVec2, unknown: &mut HashSet<TerrainId>) -> Option<usize> {
    let terrain = map.terrain(point_of(cell))?;
    if terrain == VOID {
        return None;
    }
    match item_of(terrain) {
        Some(item) => Some(item),
        None => {
            // Warned once per visit, not once per frame. Kept in the map
            // rather than dropped, so saving does not quietly delete terrain
            // this build merely cannot draw.
            if unknown.insert(terrain) {
                warn!("no art for terrain {:?}, not drawn", terrain.name());
            }
            None
        }
    }
}

/// Keep the drawn tiles equal to the terrain under the canvas.
///
/// Runs on both screens, after the view is known. The early-out on the first
/// lines is the common case by a wide margin: the camera pans at 140 canvas
/// pixels a second and a cell is 48 of them, so the window is unchanged for
/// most of every second and this system does nothing at all.
pub fn sync_tile_window(
    mut commands: Commands,
    assets: Res<AssetServer>,
    area: Res<VisibleArea>,
    current: Res<CurrentMap>,
    state: Res<State<AppState>>,
    mut window: ResMut<TileWindow>,
    mut tiles: Query<(&mut Sprite, &mut Transform, &mut Visibility), With<BackgroundTile>>,
) {
    if !area.ready {
        return;
    }
    if area.tiles == window.rect && !window.dirty && !current.is_changed() {
        return;
    }

    let wanted = area.tiles;
    let budget = park_budget(wanted.area());
    let window = &mut *window;

    // 1. Cells that left the view give their sprite back.
    window.drawn.retain(|cell, painted| {
        if wanted.contains(*cell) {
            return true;
        }
        release(
            &mut commands,
            &mut tiles,
            &mut window.free,
            painted.entity,
            budget,
        );
        false
    });

    // 2. Every cell in view shows what the map says it shows. A cell already
    //    showing the right thing is left entirely alone, which is what makes a
    //    one-column step cost one column of work.
    for cell in wanted.cells() {
        let item = wanted_item(&current.map, cell, &mut window.unknown);
        match (window.drawn.get(&cell), item) {
            // Already right.
            (Some(painted), Some(item)) if painted.item == item => {}
            // Painted over, or the map changed underneath.
            (Some(painted), Some(item)) => {
                let entity = painted.entity;
                dress(&mut tiles, &assets, entity, cell, item);
                window.drawn.insert(cell, Painted { entity, item });
            }
            // Erased to void: the cell keeps no sprite at all.
            (Some(painted), None) => {
                let entity = painted.entity;
                release(&mut commands, &mut tiles, &mut window.free, entity, budget);
                window.drawn.remove(&cell);
            }
            (None, Some(item)) => {
                let entity = match window.free.pop() {
                    Some(entity) => {
                        dress(&mut tiles, &assets, entity, cell, item);
                        entity
                    }
                    None => spawn_tile(&mut commands, &assets, cell, item, *state.get()),
                };
                window.drawn.insert(cell, Painted { entity, item });
            }
            (None, None) => {}
        }
    }

    window.rect = wanted;
    window.dirty = false;
}

/// Point an existing tile sprite at a cell — the free list's half of
/// [`spawn_tile`].
///
/// **The scale is rewritten every time**, and that is the line here that must
/// not be forgotten: the palette mixes art already drawn at cell size
/// (`floor`, scale 1.0) with art drawn at 16x16 and upscaled (`wall brown`,
/// scale 3.0), so a sprite that kept its predecessor's scale would draw at a
/// third of its size or at three times it. Visibility comes last, once the
/// sprite is both dressed and positioned, so a reused one never shows for a
/// frame where the cell it used to be in was.
fn dress(
    tiles: &mut Query<(&mut Sprite, &mut Transform, &mut Visibility), With<BackgroundTile>>,
    assets: &AssetServer,
    entity: Entity,
    cell: IVec2,
    item: usize,
) {
    let Ok((mut sprite, mut transform, mut visibility)) = tiles.get_mut(entity) else {
        return;
    };
    let art = &PALETTE[item];

    // `load` on an asset already loaded is a lookup returning the same handle,
    // so this is how a palette path becomes the handle it always is.
    let image = assets.load(art.art.path);
    if sprite.image != image {
        sprite.image = image;
    }

    let centre = cell_centre(cell);
    let at = Vec3::new(centre.x, centre.y, BACKGROUND_Z);
    // Compared before writing throughout: a mutable borrow marks a component
    // changed whether or not its value moved.
    if transform.translation != at {
        transform.translation = at;
    }
    let scale = upscale(art.scale);
    if transform.scale != scale {
        transform.scale = scale;
    }
    if *visibility != Visibility::Inherited {
        *visibility = Visibility::Inherited;
    }
}

/// Hide a tile sprite and keep it for reuse, or despawn it when the free list
/// is already as long as it is allowed to get.
fn release(
    commands: &mut Commands,
    tiles: &mut Query<(&mut Sprite, &mut Transform, &mut Visibility), With<BackgroundTile>>,
    free: &mut Vec<Entity>,
    entity: Entity,
    budget: usize,
) {
    if free.len() >= budget {
        commands.entity(entity).despawn();
        return;
    }
    if let Ok((_, _, mut visibility)) = tiles.get_mut(entity) {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
    }
    free.push(entity);
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
            Sprite::from_image(assets.load(PALETTE[item].art.path)),
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

    #[test]
    fn a_tile_is_a_character_cell_is_a_map_cell() {
        // Three names for one size: what the art is drawn to fill, what the
        // editor lays tiles out on, and what a map file's pixels divide by.
        assert_eq!(TILE, crate::characters::CELL as f32);
        assert_eq!(TILE, crate::map::PIXELS_PER_CELL as f32);
    }
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
