//! CPU-generated native 16px dirt textures, kept only for visible dirty tiles.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;

use super::actors::Sim;
use crate::characters::{ART_SCALE, depth_for, upscale};
use crate::editor::background::cell_centre;
use crate::map::Point;
use crate::render::WORLD_LAYER;
use crate::sim::dirt::Dirt;
use crate::state::AppState;
use crate::view::{ViewSystems, VisibleArea};

const SIDE: usize = 16;
const COLORS: [[u8; 3]; 4] = [
    [116, 77, 43],  // dirt
    [155, 26, 38],  // blood
    [54, 119, 163], // wet
    [38, 35, 34],   // burns
];

#[derive(Component)]
pub struct DirtTile;

struct Drawn {
    entity: Entity,
    image: Handle<Image>,
    levels: [u8; 4],
}

#[derive(Resource, Default)]
struct DirtWindow(HashMap<Point, Drawn>);

pub struct DirtPlugin;

impl Plugin for DirtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DirtWindow>()
            .add_systems(OnEnter(AppState::Game), |mut window: ResMut<DirtWindow>| {
                window.0.clear()
            })
            .add_systems(
                Update,
                sync.after(ViewSystems).run_if(in_state(AppState::Game)),
            );
    }
}

fn levels(value: Dirt) -> [u8; 4] {
    value.components().map(|v| (v * 64.0).round() as u8)
}

fn mix(mut v: u64) -> u64 {
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    v ^ (v >> 31)
}

/// Pixel positions and shades depend only on cell coordinates. Amounts change
/// coverage, never the random pattern; rendering consumes no simulation RNG.
fn pixels(cell: Point, levels: [u8; 4]) -> Vec<u8> {
    let seed = mix((cell.x as u32 as u64) << 32 | cell.y as u32 as u64);
    let mut pixels = vec![0; SIDE * SIDE * 4];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let noise = mix(seed.wrapping_add(index as u64));
        let channel = (noise & 3) as usize;
        // Each channel owns a quarter of the dots, so mixed stains retain
        // all four colors instead of one overwriting the others.
        if ((noise >> 8) & 63) >= levels[channel] as u64 {
            continue;
        }
        let shade = ((noise >> 20) % 3) as u8 * 12;
        pixel[..3].copy_from_slice(&COLORS[channel].map(|c| c.saturating_add(shade)));
        pixel[3] = if channel == 2 { 155 } else { 220 };
    }
    pixels
}

fn texture(cell: Point, levels: [u8; 4]) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: SIDE as u32,
            height: SIDE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.data = Some(pixels(cell, levels));
    image
}

fn sync(
    mut commands: Commands,
    sim: Option<Res<Sim>>,
    area: Res<VisibleArea>,
    mut images: ResMut<Assets<Image>>,
    mut window: ResMut<DirtWindow>,
) {
    let Some(sim) = sim else {
        return;
    };
    if !area.ready {
        return;
    }
    window.0.retain(|cell, drawn| {
        if area.tiles.contains(IVec2::new(cell.x, cell.y))
            && levels(sim.0.dirt().at(*cell)) != [0; 4]
        {
            return true;
        }
        commands.entity(drawn.entity).despawn();
        false
    });
    for cell in area.tiles.cells() {
        let point = Point::new(cell.x, cell.y);
        let levels = levels(sim.0.dirt().at(point));
        if levels == [0; 4] {
            continue;
        }
        if let Some(drawn) = window.0.get_mut(&point) {
            if drawn.levels != levels {
                if let Some(mut image) = images.get_mut(&drawn.image) {
                    image.data = Some(pixels(point, levels));
                }
                drawn.levels = levels;
            }
            continue;
        }
        let image = images.add(texture(point, levels));
        let centre = cell_centre(cell);
        let entity = commands
            .spawn((
                Name::new("dirt layer"),
                DirtTile,
                Sprite::from_image(image.clone()),
                // Over terrain (-1000), under props, crops and bodies.
                Transform::from_xyz(centre.x, centre.y, depth_for(centre.y) - 999.0)
                    .with_scale(upscale(ART_SCALE)),
                WORLD_LAYER,
                DespawnOnExit(AppState::Game),
            ))
            .id();
        window.0.insert(
            point,
            Drawn {
                entity,
                image,
                levels,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_are_stable_distinct_and_increase_with_amount() {
        let cell = Point::new(-3, 8);
        assert!(pixels(cell, [0; 4]).iter().all(|&v| v == 0));
        let low = pixels(cell, [16; 4]);
        let high = pixels(cell, [64; 4]);
        assert_eq!(high, pixels(cell, [64; 4]));
        assert_ne!(high, pixels(Point::new(-2, 8), [64; 4]));
        for (a, b) in low.chunks_exact(4).zip(high.chunks_exact(4)) {
            if a[3] != 0 {
                assert_eq!(a, b);
            }
        }
        let mut colors = std::collections::BTreeSet::new();
        for channel in 0..4 {
            let mut levels = [0; 4];
            levels[channel] = 64;
            let dots = pixels(cell, levels);
            let count = dots.chunks_exact(4).filter(|p| p[3] != 0).count();
            assert!(count > 30 && count < 100);
            for p in dots.chunks_exact(4).filter(|p| p[3] != 0) {
                colors.insert(p.to_vec());
            }
        }
        assert_eq!(colors.len(), 12);
    }
}
