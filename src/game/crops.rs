//! The wheat in the fields, drawn: a sprite on every crop on the canvas,
//! showing the stage it has grown to.
//!
//! A crop is drawn like a heap of scrap (`editor::scrap`) — one sprite, deep
//! by where it stands ([`depth_for`]), so somebody in front of it is drawn
//! over it — but it is the simulation's alone, read from its fields
//! ([`GameState::fields`](crate::sim::GameState::fields)), never from a map:
//! nothing is sown in the editor. **It grows without the fields changing**
//! (`sim::fields`: a stage is how long ago it was sown), so the picture is
//! checked against the clock as well as the fields every frame — a walk of
//! the few dozen crops a district grows, which is nothing — and swapped where
//! the stage moved on.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::characters::{depth_for, upscale, ART_SCALE};
use crate::editor::background::cell_centre;
use crate::map::{Bounds, Point, Size};
use crate::render::WORLD_LAYER;
use crate::sim::fields::Stage;
use crate::state::AppState;
use crate::view::{VisibleArea, ViewSystems};

use super::actors::Sim;

/// A crop at each [`Stage`], by [`Stage::number`]: seedling, young, in ear,
/// ripe. Drawn by `tools/farm_art.py`, 16x16.
pub const ART: [&str; Stage::ALL.len()] = ["wheat_1.png", "wheat_2.png", "wheat_3.png", "wheat_4.png"];

/// The crops on the canvas: cell to the sprite drawing the crop in it, and
/// the stage it shows.
#[derive(Resource, Default)]
struct CropWindow {
    drawn: HashMap<Point, (Entity, Stage)>,
}

pub struct CropsPlugin;

impl Plugin for CropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CropWindow>()
            // Forgotten on the way in, not out: the sprites go with
            // `DespawnOnExit`, which closing the window skips `OnExit` for.
            .add_systems(OnEnter(AppState::Game), |mut window: ResMut<CropWindow>| window.drawn.clear())
            .add_systems(Update, sync_crops.after(ViewSystems).run_if(in_state(AppState::Game)));
    }
}

/// Keep a sprite on every crop whose cell is on the canvas, at the stage it
/// has grown to, and on no other.
fn sync_crops(
    mut commands: Commands,
    assets: Res<AssetServer>,
    area: Res<VisibleArea>,
    sim: Option<Res<Sim>>,
    mut window: ResMut<CropWindow>,
    mut sprites: Query<&mut Sprite>,
    mut seen: Local<Vec<Point>>,
) {
    let Some(sim) = sim else {
        return;
    };
    if !area.ready || area.tiles.is_empty() {
        return;
    }
    let tiles = area.tiles;
    let canvas = Bounds::new(
        Point::new(tiles.min.x, tiles.min.y),
        Size::new(tiles.max.x - tiles.min.x + 1, tiles.max.y - tiles.min.y + 1),
    );
    let now = sim.0.clock();
    seen.clear();
    for crop in sim.0.fields().iter().filter(|crop| canvas.contains(crop.cell)) {
        let stage = crop.stage(now);
        let art = || assets.load(ART[stage.number() as usize - 1]);
        seen.push(crop.cell);
        match window.drawn.get_mut(&crop.cell) {
            Some((_, shown)) if *shown == stage => {}
            Some((entity, shown)) => {
                if let Ok(mut sprite) = sprites.get_mut(*entity) {
                    sprite.image = art();
                }
                *shown = stage;
            }
            None => {
                let centre = cell_centre(IVec2::new(crop.cell.x, crop.cell.y));
                let entity = commands
                    .spawn((
                        Name::new("crop"),
                        Sprite::from_image(art()),
                        Transform::from_xyz(centre.x, centre.y, depth_for(centre.y)).with_scale(upscale(ART_SCALE)),
                        WORLD_LAYER,
                        DespawnOnExit(AppState::Game),
                    ))
                    .id();
                window.drawn.insert(crop.cell, (entity, stage));
            }
        }
    }
    // Harvested, or off the canvas. `seen` is in cell order, as the fields are.
    window.drawn.retain(|cell, (entity, _)| {
        if seen.binary_search(cell).is_ok() {
            return true;
        }
        commands.entity(*entity).despawn();
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stage has a picture, a 16x16 PNG drawn at the art resolution.
    #[test]
    fn every_stage_of_a_crop_has_its_own_art_at_the_art_resolution() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for (stage, path) in Stage::ALL.iter().zip(ART) {
            let bytes = std::fs::read(assets.join(path)).unwrap_or_else(|_| panic!("{}: no {path}", stage.name()));
            let (width, height) = (
                u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
                u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
            );
            assert_eq!((width, height), (16, 16), "{path}");
        }
    }
}
