//! The outposts, drawn: every outpost's flag, the building tiles of what is
//! going up, and what is built — its walls, its doorway, its fridge, its
//! local warehouse and its beds — wherever they are on the canvas.
//!
//! Like the crops (`game::crops`) they are the simulation's alone, read from
//! [`GameState::outposts`](crate::sim::GameState::outposts), never from a
//! map: nothing of an outpost is in a map file or the editor's copy of the
//! map, the walls included, so this draws them all. **What is built is drawn
//! in the palette's own art**, found by name (`wall brown`, `fridge`, `bed 1`,
//! `outpost store`, `door`), and the flag and the building tiles in their own
//! (`tools/outpost_art.py`). A door is drawn as open as the simulation says,
//! the store as full, and a building tile shows how far it has got, so the
//! picture is checked every frame — a walk of the outposts' twenty-odd tiles,
//! which is nothing — and a sprite drawn again where it changed.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::characters::{depth_for, upscale, ART_SCALE};
use crate::editor::background::{cell_centre, PALETTE as TILES};
use crate::editor::props::{DoorLeaf, PALETTE as PROPS};
use crate::editor::PaletteItem;
use crate::map::{Bounds, Point, Size};
use crate::render::WORLD_LAYER;
use crate::sim::outposts::{Part, Stage};
use crate::state::AppState;
use crate::view::{ViewSystems, VisibleArea};

use super::actors::Sim;
use super::props::bin_frame;

/// An outpost's flag, at its middle: drawn by `tools/outpost_art.py`, 16x16.
pub const FLAG: &str = "outpost_flag.png";

/// A building tile, in four stages as the work on it goes: a strip of four
/// 16x16 frames, drawn by `tools/outpost_art.py`.
pub const BUILDING_TILE: &str = "building_tile.png";

/// How many stages [`BUILDING_TILE`] shows.
const BUILDING_FRAMES: u32 = 4;

/// What an outpost's walls are drawn as: a wall of the editor's palette.
const WALL: &str = "wall brown";

/// How far above the ground under it a built wall is drawn: over the tile it
/// stands on, under anybody walking past — it is terrain, as the map says.
const WALL_DEPTH: f32 = -990.0;

/// What a cell of an outpost shows, so a sprite is drawn again only when it
/// changes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Look {
    Flag,
    /// A building tile, at this stage.
    Laid(u32),
    /// Built, showing this frame of its art (a door's openness, a store's
    /// fill; 0 for the rest).
    Built(Part, u32),
}

/// The outposts' cells on the canvas: cell to the sprite drawing it, and what
/// it shows.
#[derive(Resource, Default)]
struct OutpostWindow {
    drawn: HashMap<Point, (Entity, Look)>,
}

pub struct OutpostsPlugin;

impl Plugin for OutpostsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OutpostWindow>()
            // Forgotten on the way in, not out: the sprites go with
            // `DespawnOnExit`, which closing the window skips `OnExit` for.
            .add_systems(OnEnter(AppState::Game), |mut window: ResMut<OutpostWindow>| window.drawn.clear())
            .add_systems(Update, sync_outposts.after(ViewSystems).run_if(in_state(AppState::Game)));
    }
}

/// The palette entry `name`, in `palette`.
fn item(palette: &'static [PaletteItem], name: &str) -> &'static PaletteItem {
    palette.iter().find(|item| item.name == name).unwrap_or_else(|| panic!("no {name} on the palette"))
}

/// Frame `frame` of a strip of `side`-texel frames.
fn frame_of(side: f32, frame: u32) -> Rect {
    Rect::new(frame as f32 * side, 0.0, (frame + 1) as f32 * side, side)
}

/// Keep a sprite on every outpost cell on the canvas that has something to
/// show — a flag, a building tile, something built — as it is now, and on no
/// other.
fn sync_outposts(
    mut commands: Commands,
    assets: Res<AssetServer>,
    area: Res<VisibleArea>,
    sim: Option<Res<Sim>>,
    mut window: ResMut<OutpostWindow>,
    mut seen: Local<Vec<Point>>,
    mut doors: Local<Vec<Point>>,
) {
    let Some(sim) = sim else {
        return;
    };
    if !area.ready || area.tiles.is_empty() {
        return;
    }
    let tiles = area.tiles;
    let canvas = Bounds::new(Point::new(tiles.min.x, tiles.min.y), Size::new(tiles.max.x - tiles.min.x + 1, tiles.max.y - tiles.min.y + 1));
    let state = &sim.0;
    seen.clear();
    for outpost in state.outposts().iter().filter(|outpost| outpost.footprint().overlaps(canvas)) {
        doors.clear();
        doors.extend(outpost.tiles().iter().filter(|tile| tile.part == Part::Door && tile.stage == Stage::Built).map(|tile| tile.cell));
        let flag = std::iter::once((outpost.center(), Look::Flag));
        let looks = outpost.tiles().iter().filter_map(|tile| {
            let look = match tile.stage {
                Stage::Planned => return None,
                Stage::Laid { .. } => Look::Laid(((tile.done() * BUILDING_FRAMES as f32) as u32).min(BUILDING_FRAMES - 1)),
                Stage::Built => {
                    let frame = match tile.part {
                        Part::Door => {
                            let last = item(PROPS, "door").art.frames - 1;
                            ((state.doors().openness(tile.cell) * last as f32).round() as u32).min(last)
                        }
                        Part::Store => bin_frame(state.bins().stored(tile.cell)),
                        _ => 0,
                    };
                    Look::Built(tile.part, frame)
                }
            };
            Some((tile.cell, look))
        });
        for (cell, look) in flag.chain(looks).filter(|&(cell, _)| canvas.contains(cell)) {
            seen.push(cell);
            if window.drawn.get(&cell).is_some_and(|&(_, shown)| shown == look) {
                continue;
            }
            if let Some((entity, _)) = window.drawn.remove(&cell) {
                commands.entity(entity).despawn();
            }
            let entity = spawn(&mut commands, &assets, state, cell, look, &doors);
            window.drawn.insert(cell, (entity, look));
        }
    }
    // Gone off the canvas. Unsorted, but a few dozen at the most.
    window.drawn.retain(|cell, (entity, _)| {
        if seen.contains(cell) {
            return true;
        }
        commands.entity(*entity).despawn();
        false
    });
}

/// The sprite for `cell` showing `look`. `doors` are the outpost's built
/// doors, for which way a door's leaf slides.
fn spawn(commands: &mut Commands, assets: &AssetServer, state: &crate::sim::GameState, cell: Point, look: Look, doors: &[Point]) -> Entity {
    let centre = cell_centre(IVec2::new(cell.x, cell.y));
    let (image, rect, scale, depth, rotation, flip_x) = match look {
        Look::Flag => (assets.load(FLAG), None, ART_SCALE, 0.0, Quat::IDENTITY, false),
        Look::Laid(stage) => {
            (assets.load(BUILDING_TILE), Some(frame_of(16.0, stage)), ART_SCALE, 0.0, Quat::IDENTITY, false)
        }
        Look::Built(Part::Wall, _) => {
            let wall = item(TILES, WALL);
            (assets.load(wall.art.path), wall.first_frame(), wall.scale, WALL_DEPTH, Quat::IDENTITY, false)
        }
        Look::Built(part, frame) => {
            let prop = item(PROPS, part.prop().expect("built furniture is a prop"));
            let rect = (prop.art.frames > 1).then(|| frame_of(prop.frame(), frame));
            let (rotation, mirrored) = if part == Part::Door {
                let (vertical, mirrored) = DoorLeaf::pose(&state.map, cell, doors);
                (if vertical { Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY }, mirrored)
            } else {
                (Quat::IDENTITY, false)
            };
            (assets.load(prop.art.path), rect, prop.scale, 0.0, rotation, mirrored)
        }
    };
    commands
        .spawn((
            Name::new("outpost"),
            Sprite { image, rect, flip_x, ..default() },
            Transform::from_xyz(centre.x, centre.y, depth_for(centre.y) + depth).with_scale(upscale(scale)).with_rotation(rotation),
            WORLD_LAYER,
            DespawnOnExit(AppState::Game),
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag and the building tile have art of their own, at the art
    /// resolution: the flag one 16x16 frame, the building tile four.
    #[test]
    fn the_flag_and_the_building_tile_have_their_own_art_at_the_art_resolution() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for (path, frames) in [(FLAG, 1), (BUILDING_TILE, BUILDING_FRAMES)] {
            let bytes = std::fs::read(assets.join(path)).unwrap_or_else(|_| panic!("no {path}"));
            let (width, height) = (u32::from_be_bytes(bytes[16..20].try_into().unwrap()), u32::from_be_bytes(bytes[20..24].try_into().unwrap()));
            assert_eq!((width, height), (16 * frames, 16), "{path}");
        }
    }

    /// Everything an outpost builds is drawn in art the palettes have.
    #[test]
    fn everything_built_has_art_on_a_palette() {
        let _ = item(TILES, WALL);
        for part in [Part::Door, Part::Fridge, Part::Store, Part::Bed] {
            let _ = item(PROPS, part.prop().unwrap());
        }
    }
}
