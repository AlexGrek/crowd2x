//! Visible flower beds, drawn at native resolution with the same culling as crops.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::characters::{ART_SCALE, depth_for, upscale};
use crate::editor::background::cell_centre;
use crate::map::{Bounds, Point, Size};
use crate::render::WORLD_LAYER;

use crate::state::AppState;
use crate::view::{ViewSystems, VisibleArea};

use super::actors::Sim;

/// Six varieties, each with a seedling and a flowering picture.
pub const ART: [[&str; 2]; 6] = [
    ["flowers_0_0.png", "flowers_0_1.png"],
    ["flowers_1_0.png", "flowers_1_1.png"],
    ["flowers_2_0.png", "flowers_2_1.png"],
    ["flowers_3_0.png", "flowers_3_1.png"],
    ["flowers_4_0.png", "flowers_4_1.png"],
    ["flowers_5_0.png", "flowers_5_1.png"],
];

/// The flower beds on the canvas, with their variety and growth stage.
#[derive(Resource, Default)]
struct FlowerWindow {
    drawn: HashMap<Point, (Entity, (u8, bool))>,
}

pub struct FlowersPlugin;

impl Plugin for FlowersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlowerWindow>()
            // Forgotten on the way in, not out: the sprites go with
            // `DespawnOnExit`, which closing the window skips `OnExit` for.
            .add_systems(
                OnEnter(AppState::Game),
                |mut window: ResMut<FlowerWindow>| window.drawn.clear(),
            )
            .add_systems(
                Update,
                sync_flowers
                    .after(ViewSystems)
                    .run_if(in_state(AppState::Game)),
            );
    }
}

/// Keep a sprite on each visible bed, swapping its art as it blooms.
fn sync_flowers(
    mut commands: Commands,
    assets: Res<AssetServer>,
    area: Res<VisibleArea>,
    sim: Option<Res<Sim>>,
    mut window: ResMut<FlowerWindow>,
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
    for crop in sim
        .0
        .flowers()
        .iter()
        .filter(|crop| canvas.contains(crop.cell))
    {
        let stage = (crop.variety, crop.blooming(now));
        let art = || assets.load(ART[stage.0 as usize][stage.1 as usize]);
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
                        Name::new("flower bed"),
                        Sprite::from_image(art()),
                        Transform::from_xyz(centre.x, centre.y, depth_for(centre.y))
                            .with_scale(upscale(ART_SCALE)),
                        WORLD_LAYER,
                        DespawnOnExit(AppState::Game),
                    ))
                    .id();
                window.drawn.insert(crop.cell, (entity, stage));
            }
        }
    }
    // Off the canvas. `seen` is in cell order, as the flower beds are.
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

    #[test]
    fn every_flower_variety_has_distinct_native_resolution_art() {
        assert_eq!(ART.len(), crate::sim::flowers::VARIETIES);
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut blooms = std::collections::BTreeSet::new();
        for stages in ART {
            for path in stages {
                let bytes = std::fs::read(assets.join(path)).expect(path);
                assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 16);
                assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 16);
            }
            blooms.insert(std::fs::read(assets.join(stages[1])).unwrap());
        }
        assert_eq!(
            blooms.len(),
            ART.len(),
            "each mature variety must look different"
        );
    }
}
