//! Pixel sparks on failure, and a red fault marker until the box is repaired.

use super::actors::Sim;
use crate::characters::{ART_SCALE, depth_for};
use crate::editor::background::TILE;
use crate::map::Point;
use crate::render::WORLD_LAYER;
use crate::state::AppState;
use bevy::prelude::*;

#[derive(Component)]
struct Fault(Point);

#[derive(Component)]
struct Spark {
    origin: Vec2,
    velocity: Vec2,
    age: f32,
    life: f32,
}

pub struct ElectricalPlugin;

impl Plugin for ElectricalPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (faults, sparks)
                .chain()
                .in_set(super::SpriteSync)
                .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
        );
    }
}

fn faults(mut commands: Commands, sim: Res<Sim>, marks: Query<(Entity, &Fault)>) {
    for (entity, fault) in &marks {
        if !sim.0.box_is_damaged(fault.0) {
            commands.entity(entity).despawn();
        }
    }
    for &at in sim.0.electrical().damaged() {
        if marks.iter().any(|(_, fault)| fault.0 == at) {
            continue;
        }
        let centre = Vec2::new(at.x as f32 + 0.5, at.y as f32 + 0.5) * TILE;
        // A persistent red cross, built from true art-sized pixel squares.
        commands
            .spawn((
                Fault(at),
                Transform::from_translation(centre.extend(depth_for(centre.y) + 700.0)),
                Visibility::default(),
                DespawnOnExit(AppState::Game),
            ))
            .with_children(|parent| {
                for y in -2i32..=2 {
                    for x in -2i32..=2 {
                        if x.abs() == y.abs() {
                            parent.spawn((
                                Sprite::from_color(
                                    Color::srgb(1.0, 0.22, 0.08),
                                    Vec2::splat(ART_SCALE),
                                ),
                                Transform::from_xyz(
                                    x as f32 * ART_SCALE + 0.5,
                                    y as f32 * ART_SCALE + 0.5,
                                    0.0,
                                ),
                                WORLD_LAYER,
                            ));
                        }
                    }
                }
            });
        for i in 0..18 {
            let angle = i as f32 * std::f32::consts::TAU / 18.0;
            let speed = 25.0 + (i % 4) as f32 * 13.0;
            commands.spawn((
                Spark {
                    origin: centre,
                    velocity: Vec2::new(angle.cos(), angle.sin()) * speed,
                    age: 0.0,
                    life: 0.55 + (i % 3) as f32 * 0.12,
                },
                Sprite::from_color(Color::srgb(1.0, 0.9, 0.25), Vec2::splat(ART_SCALE)),
                Transform::from_translation(
                    (centre + Vec2::splat(0.5)).extend(depth_for(centre.y) + 701.0),
                ),
                WORLD_LAYER,
                DespawnOnExit(AppState::Game),
            ));
        }
    }
}

fn sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut sparks: Query<(Entity, &mut Spark, &mut Transform, &mut Sprite)>,
) {
    for (entity, mut spark, mut transform, mut sprite) in &mut sparks {
        spark.age += time.delta_secs();
        if spark.age >= spark.life {
            commands.entity(entity).despawn();
            continue;
        }
        let age = spark.age;
        let position = spark.origin + spark.velocity * age - Vec2::Y * (45.0 * age * age);
        // Odd-sized sprites need a half-pixel nudge for whole-pixel edges.
        let position = (position / ART_SCALE).round() * ART_SCALE + Vec2::splat(0.5);
        transform.translation.x = position.x;
        transform.translation.y = position.y;
        sprite.color = if age / spark.life < 0.45 {
            Color::srgb(1.0, 0.95, 0.5)
        } else {
            Color::srgb(1.0, 0.4, 0.05)
        };
    }
}
