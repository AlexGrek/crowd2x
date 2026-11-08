//! A robot's end, drawn: a fireball, sparks flying out and a puff of smoke.
//!
//! What blew up where is the simulation's ([`Robots::explosions`], each with
//! a serial); this plays each one it has not seen yet. **Real time**, like the
//! small talk's sparks: an explosion is a reading, not a fact about the world.
//! The fireball is the eight 16x16 frames of `explosion.png` shown at an
//! integer three times the art's scale; sparks are plain colours placed on a
//! whole canvas pixel by their corner every frame; the smoke rises and fades.
//! All on [`WORLD_LAYER`], gone by `DespawnOnExit`.
//!
//! [`Robots::explosions`]: crate::sim::robots::Robots::explosions

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::characters::{depth_for, snap_to_texel, ART_SCALE};
use crate::render::WORLD_LAYER;
use crate::state::AppState;

use super::actors::{world_pos, Sim};

const FIREBALL: &str = "explosion.png";
const SMOKE: &str = "smoke.png";
const FRAMES: u32 = 8;
const FIREBALL_SECONDS: f32 = 0.7;
const SMOKE_SECONDS: f32 = 2.0;
const SPARKS: usize = 8;
const SPARK_SECONDS: f32 = 0.8;
/// Canvas pixels a spark flies out: four cells.
const SPARK_REACH: f32 = 4.0 * crate::editor::background::TILE;
/// The integer a fireball is enlarged by over the art's own scale.
const BIG: f32 = 3.0;
const DEPTH: f32 = 0.009;

#[derive(Resource, Default)]
struct Seen(u64);

#[derive(Component)]
struct Fireball {
    age: f32,
}

#[derive(Component)]
struct Debris {
    from: Vec2,
    to: Vec2,
    age: f32,
    depth: f32,
}

#[derive(Component)]
struct Puff {
    from: Vec2,
    age: f32,
    depth: f32,
}

pub struct ExplosionsPlugin;

impl Plugin for ExplosionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Seen>()
            .add_systems(OnExit(AppState::Game), |mut seen: ResMut<Seen>| seen.0 = 0)
            .add_systems(
                Update,
                (set_off, burn, fly, rise)
                    .chain()
                    .in_set(super::SpriteSync)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            );
    }
}

/// Play every explosion the simulation reports that has not been seen.
fn set_off(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
    sim: Res<Sim>,
    mut seen: ResMut<Seen>,
) {
    let robots = &sim.0.robots();
    for explosion in robots.explosions() {
        if explosion.serial <= seen.0 {
            continue;
        }
        seen.0 = explosion.serial;
        let centre = world_pos((explosion.at.x as f32 + 0.5, explosion.at.y as f32 + 0.5));
        let depth = depth_for(centre.y) + DEPTH;
        let layout = layouts.add(TextureAtlasLayout::from_grid(UVec2::splat(16), FRAMES, 1, None, None));
        commands.spawn((
            Name::new("fireball"),
            Sprite {
                image: assets.load(FIREBALL),
                texture_atlas: Some(TextureAtlas { layout, index: 0 }),
                custom_size: Some(Vec2::splat(16.0 * ART_SCALE * BIG)),
                ..default()
            },
            Transform::from_translation(centre.extend(depth)),
            WORLD_LAYER,
            DespawnOnExit(AppState::Game),
            Fireball { age: 0.0 },
        ));
        commands.spawn((
            Name::new("smoke"),
            Sprite { image: assets.load(SMOKE), custom_size: Some(Vec2::splat(16.0 * ART_SCALE)), ..default() },
            Transform::from_translation(centre.extend(depth - 0.001)),
            WORLD_LAYER,
            DespawnOnExit(AppState::Game),
            Puff { from: centre, age: 0.0, depth: depth - 0.001 },
        ));
        for i in 0..SPARKS {
            let angle = (i as f32 + explosion.serial as f32 * 0.37) / SPARKS as f32 * std::f32::consts::TAU;
            let to = centre + Vec2::from_angle(angle) * SPARK_REACH;
            commands.spawn((
                Name::new("debris"),
                Sprite::from_color(Color::srgb(1.0, 0.8, 0.25), Vec2::splat(ART_SCALE)),
                Anchor::BOTTOM_LEFT,
                Transform::from_translation(centre.extend(depth + 0.001)),
                WORLD_LAYER,
                DespawnOnExit(AppState::Game),
                Debris { from: centre, to, age: 0.0, depth: depth + 0.001 },
            ));
        }
    }
}

fn burn(mut commands: Commands, time: Res<Time>, mut fireballs: Query<(Entity, &mut Fireball, &mut Sprite)>) {
    for (id, mut fireball, mut sprite) in &mut fireballs {
        fireball.age += time.delta_secs();
        if fireball.age >= FIREBALL_SECONDS {
            commands.entity(id).despawn();
            continue;
        }
        let frame = ((fireball.age / FIREBALL_SECONDS) * FRAMES as f32) as usize;
        if let Some(atlas) = sprite.texture_atlas.as_mut() {
            atlas.index = frame.min(FRAMES as usize - 1);
        }
    }
}

fn fly(mut commands: Commands, time: Res<Time>, mut sparks: Query<(Entity, &mut Debris, &mut Transform, &mut Sprite)>) {
    for (id, mut debris, mut transform, mut sprite) in &mut sparks {
        debris.age += time.delta_secs();
        let p = debris.age / SPARK_SECONDS;
        if p >= 1.0 {
            commands.entity(id).despawn();
            continue;
        }
        // Out fast, slowing: ease-out.
        let out = 1.0 - (1.0 - p) * (1.0 - p);
        let at = snap_to_texel(debris.from.lerp(debris.to, out));
        transform.translation = at.extend(debris.depth);
        sprite.color = Color::srgb(1.0, 0.8 - 0.6 * p, 0.25 - 0.2 * p).with_alpha(1.0 - p * p);
    }
}

fn rise(mut commands: Commands, time: Res<Time>, mut puffs: Query<(Entity, &mut Puff, &mut Transform, &mut Sprite)>) {
    for (id, mut puff, mut transform, mut sprite) in &mut puffs {
        puff.age += time.delta_secs();
        let p = puff.age / SMOKE_SECONDS;
        if p >= 1.0 {
            commands.entity(id).despawn();
            continue;
        }
        let at = snap_to_texel(puff.from + Vec2::new(0.0, p * 2.0 * ART_SCALE * 16.0 / 2.0));
        transform.translation = at.extend(puff.depth);
        sprite.color = Color::WHITE.with_alpha(0.8 * (1.0 - p));
    }
}
