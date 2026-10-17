//! The heaps of scrap metal out in the wasteland, drawn on both map screens.
//!
//! A heap is drawn like a prop: one sprite, deep by where it stands
//! ([`depth_for`]), so somebody walking in front of it is drawn over it and
//! somebody behind it under it. But it is not in any object layer. It is the
//! wasteland's (`map::wasteland`), read out of the map a chunk at a time
//! ([`Map::heaps`]), and on the game screen it can be salvaged away — so
//! there the heaps are read from the **simulation's** map, the one copy
//! anybody empties, and not from `CurrentMap`, where every heap is as it was
//! found. In the editor they are dimmed with the rest of the wasteland
//! ([`super::background::OFF_SITE_TINT`]).
//!
//! Windowed to the canvas like the tiles and the props ([`ScrapWindow`]):
//! looked at again only when the view crosses a cell boundary or a piece of
//! scrap was taken somewhere ([`Map::salvage_revision`]), and a heap already
//! drawn is left alone. Not pooled, for the props' reason — a yard on screen
//! is a few dozen heaps at most, and heaps come and go a column at a time.

use std::collections::HashMap;

use bevy::prelude::*;

use super::background::{cell_centre, OFF_SITE_TINT};
use super::CurrentMap;
use crate::characters::{depth_for, upscale, ART_SCALE};
use crate::game::actors::Sim;
use crate::map::wasteland::SCRAP_VARIANTS;
use crate::map::{Bounds, Map, Point, Scrap, Size};
use crate::render::WORLD_LAYER;
use crate::state::AppState;
use crate::view::VisibleArea;
use crate::view::CellRect;

/// The picture of each variant of a heap, by [`Scrap::variant`]. Drawn by
/// `tools/scrap_art.py`, 16x16.
pub const ART: [&str; SCRAP_VARIANTS as usize] =
    ["scrap_1.png", "scrap_2.png", "scrap_3.png", "scrap_4.png", "scrap_5.png"];

/// A heap of scrap on the canvas, and the cell it stands in.
#[derive(Component)]
pub struct Heap {
    pub cell: Point,
}

/// The heaps on the canvas.
#[derive(Resource, Default)]
pub struct ScrapWindow {
    /// Cell to the sprite drawing the heap in it.
    drawn: HashMap<Point, Entity>,
    /// What `drawn` covers, so a camera that has not crossed a cell boundary
    /// does no work.
    rect: CellRect,
    /// The salvage revision of the map it was drawn from, so a heap emptied
    /// under the canvas is taken off it. `None` until the first fill.
    revision: Option<u64>,
}

impl ScrapWindow {
    /// Forget everything without despawning it — the sprites carry
    /// `DespawnOnExit`, as [`super::background::TileWindow::clear`] explains.
    pub fn clear(&mut self) {
        self.drawn.clear();
        self.rect = CellRect::EMPTY;
        self.revision = None;
    }
}

/// The cells of a non-empty [`CellRect`] as [`Bounds`].
fn bounds_of(tiles: CellRect) -> Bounds {
    Bounds::new(
        Point::new(tiles.min.x, tiles.min.y),
        Size::new(tiles.max.x - tiles.min.x + 1, tiles.max.y - tiles.min.y + 1),
    )
}

/// Keep a sprite on every heap with anything left in it whose cell is on the
/// canvas, and on no other.
///
/// After the chunks under the canvas are loaded (`background::stream_chunks`),
/// though a heap whose chunk is not loaded is drawn all the same: the map
/// works it out ([`Map::heaps`]).
#[allow(clippy::too_many_arguments)]
pub fn sync_scrap_window(
    mut commands: Commands,
    assets: Res<AssetServer>,
    area: Res<VisibleArea>,
    current: Res<CurrentMap>,
    sim: Option<Res<Sim>>,
    state: Res<State<AppState>>,
    mut window: ResMut<ScrapWindow>,
    mut wanted: Local<HashMap<Point, Scrap>>,
) {
    if !area.ready || area.tiles.is_empty() {
        return;
    }
    let editing = *state.get() == AppState::Editor;
    // The simulation's map in the game, where heaps are salvaged; the map
    // being edited otherwise, where none ever is.
    let map: &Map = match sim.as_deref() {
        Some(sim) if !editing => &sim.0.map,
        _ => &current.map,
    };
    let revision = map.salvage_revision();
    if area.tiles == window.rect && window.revision == Some(revision) {
        return;
    }

    wanted.clear();
    map.heaps(bounds_of(area.tiles), |cell, heap| {
        wanted.insert(cell, heap);
    });

    // Heaps that left the view, or were emptied.
    window.drawn.retain(|cell, entity| {
        if wanted.contains_key(cell) {
            return true;
        }
        commands.entity(*entity).despawn();
        false
    });

    let tint = if editing { OFF_SITE_TINT } else { Color::WHITE };
    for (&cell, heap) in wanted.iter() {
        if window.drawn.contains_key(&cell) {
            continue;
        }
        let centre = cell_centre(IVec2::new(cell.x, cell.y));
        let entity = commands
            .spawn((
                Name::new("scrap"),
                Heap { cell },
                Sprite {
                    color: tint,
                    ..Sprite::from_image(assets.load(ART[heap.variant as usize % ART.len()]))
                },
                Transform::from_xyz(centre.x, centre.y, depth_for(centre.y)).with_scale(upscale(ART_SCALE)),
                WORLD_LAYER,
                DespawnOnExit(*state.get()),
            ))
            .id();
        window.drawn.insert(cell, entity);
    }

    window.rect = area.tiles;
    window.revision = Some(revision);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant the wasteland can lay has a picture, and the picture
    /// is a 16x16 PNG drawn at the art's own resolution.
    #[test]
    fn every_variant_of_a_heap_has_its_own_art_at_the_art_resolution() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for (variant, path) in ART.iter().enumerate() {
            let file = assets.join(path);
            let bytes = std::fs::read(&file).unwrap_or_else(|_| panic!("variant {variant}: no {}", file.display()));
            // A PNG's width and height are the first two words of its IHDR.
            let (width, height) = (
                u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
                u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
            );
            assert_eq!((width, height), (16, 16), "{path}");
        }
        let mut names = ART.to_vec();
        names.dedup();
        assert_eq!(names.len(), SCRAP_VARIANTS as usize, "five different pictures");
    }
}
