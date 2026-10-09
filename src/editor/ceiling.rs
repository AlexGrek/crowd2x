//! The ceiling layer: which cells have a roof over them.
//!
//! The ceiling is never drawn in the game — the view is from above, through
//! it — and what it does is lighting's business: the sun reaches what has no
//! roof and spills in under what does (`crate::lighting`). So the editor is the
//! one place it is seen, as a translucent hatch over every roofed cell, and
//! only while the ceiling is the layer being edited, where the hatch would
//! otherwise hide the floor everybody else is painting.
//!
//! Painted corner to corner, like a block: a room is roofed in one drag, and a
//! right drag opens it to the sky.

use bevy::prelude::*;

use super::{background, CurrentMap, Layer, PaletteItem, Tool};
use crate::characters::upscale;
use crate::map::Point;
use crate::render::WORLD_LAYER;
use crate::state::AppState;
use crate::view::{CellRect, VisibleArea};

/// The layer has one thing to paint: a roof. Erasing is opening it.
pub const PALETTE: &[PaletteItem] = &[PaletteItem::upscaled("ceiling", "ceiling.png")];

/// Above every tile and prop, under the lamps (which hang below it) and the
/// cursor.
const OVERLAY_Z: f32 = 40.0;

/// The hatch over the roofed cells on the canvas, while the ceiling layer is
/// being edited.
///
/// Rebuilt outright when the view crosses a cell or the ceiling changes —
/// a canvas is a few hundred cells at most, and this runs on those frames
/// only — rather than pooled the way terrain is: it exists only while one
/// layer of one screen is up.
#[derive(Resource, Default)]
pub struct CeilingOverlay {
    drawn: Vec<Entity>,
    rect: CellRect,
    dirty: bool,
}

impl CeilingOverlay {
    /// Say the ceiling changed, as the map windows' `touch` does.
    pub fn touch(&mut self) {
        self.dirty = true;
    }

    fn clear(&mut self, commands: &mut Commands) {
        for entity in self.drawn.drain(..) {
            commands.entity(entity).despawn();
        }
        self.rect = CellRect::EMPTY;
    }
}

/// Roof a cell over, or open it. Off the map is refused, like painting.
pub fn set(overlay: &mut CeilingOverlay, map: &mut crate::map::Map, cell: IVec2, roofed: bool) {
    let point = Point::new(cell.x, cell.y);
    if map.has_ceiling(point) != roofed && map.set_ceiling(point, roofed) {
        overlay.touch();
    }
}

pub fn sync_ceiling_overlay(
    mut commands: Commands,
    assets: Res<AssetServer>,
    area: Res<VisibleArea>,
    current: Res<CurrentMap>,
    tool: Res<Tool>,
    mut overlay: ResMut<CeilingOverlay>,
) {
    if tool.layer() != Layer::Ceiling {
        if !overlay.drawn.is_empty() {
            overlay.clear(&mut commands);
        }
        return;
    }
    if !area.ready || (area.tiles == overlay.rect && !overlay.dirty && !tool.is_changed()) {
        return;
    }
    overlay.clear(&mut commands);
    overlay.dirty = false;

    let item = &PALETTE[0];
    let image: Handle<Image> = assets.load(item.art.path);
    let tiles = area.tiles;
    for y in tiles.min.y..=tiles.max.y {
        for x in tiles.min.x..=tiles.max.x {
            if !current.map.has_ceiling(Point::new(x, y)) {
                continue;
            }
            let centre = background::cell_centre(IVec2::new(x, y));
            let entity = commands
                .spawn((
                    Name::new("ceiling"),
                    Sprite::from_image(image.clone()),
                    Transform::from_xyz(centre.x, centre.y, OVERLAY_Z).with_scale(upscale(item.scale)),
                    WORLD_LAYER,
                    DespawnOnExit(AppState::Editor),
                ))
                .id();
            overlay.drawn.push(entity);
        }
    }
    overlay.rect = tiles;
}

/// Forget the hatch without despawning it: entering the editor, where the
/// sprites from last time went with `DespawnOnExit`.
pub fn reset(mut overlay: ResMut<CeilingOverlay>) {
    overlay.drawn.clear();
    overlay.rect = CellRect::EMPTY;
    overlay.dirty = true;
}
