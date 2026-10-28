//! Small talk, drawn: sparks flying between two people making it.
//!
//! What a small talk *is* — who with, since when, whether the other has
//! answered — is the simulation's ([`GameEntity::smalltalk`]); this only
//! shows it. Anybody on the canvas in an answered small talk throws a spark
//! from its head at the other's every [`SPARK_EVERY`] seconds: a fleck of one
//! texel that flies an arc over to the other's head, cooling from white
//! through yellow and orange to red as it goes, shrinking to a pixel and
//! fading out — so a pair talking is a little shower of sparks crossing
//! between them, both ways.
//!
//! **Real time**, [`Time`] and not the fixed step, like the goal emoji's fade:
//! a spark is a reading, not a fact about the world, and it keeps flying while
//! the game is paused (and a small talk paused stays a small talk). Each spark
//! is a plain colour, placed on a whole canvas pixel by its corner every frame
//! ([`Anchor::BOTTOM_LEFT`]), so none ever lands between two. There are never
//! more than [`MOST_SPARKS`] at once, whatever the crowd: a spark is a sprite,
//! and a district of talkers on the canvas at the widest zoom would otherwise
//! be thousands.
//!
//! [`GameEntity::smalltalk`]: crate::sim::GameEntity::smalltalk

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

use crate::characters::{depth_for, CELL};
use crate::render::WORLD_LAYER;
use crate::sim::Uid;
use crate::state::AppState;

use super::actors::{world_pos, Sim, VisibleCrowd};

/// Seconds between one talker's sparks.
const SPARK_EVERY: f32 = 0.18;

/// How long a spark flies, in seconds: between these two.
const SHORTEST_FLIGHT: f32 = 0.55;
const LONGEST_FLIGHT: f32 = 0.85;

/// Where a spark leaves from and lands, over the middle of a person's cell,
/// in canvas pixels: about the mouth.
const MOUTH: f32 = CELL as f32 / 2.0 - 10.0;

/// Canvas pixels a second a spark falls by, a second: what bends its flight
/// into an arc.
const GRAVITY: f32 = 160.0;

/// How far a spark strays, sideways and up, in canvas pixels a second.
const STRAY: f32 = 18.0;

/// The most sparks in the air at once.
const MOST_SPARKS: usize = 120;

/// In front of everything about a character — the paperdoll, the selection
/// frame, a held item, the action bar (0.006) and the goal emoji (0.007) —
/// and inside the 0.01 [`depth_for`] leaves between one pixel of Y and the
/// next.
const SPARK_DEPTH: f32 = 0.008;

/// A spark's colours as it cools, white hot to red.
const COOLING: [Color; 4] = [
    Color::srgb(1.0, 0.98, 0.85),
    Color::srgb(1.0, 0.85, 0.25),
    Color::srgb(1.0, 0.5, 0.1),
    Color::srgb(0.8, 0.15, 0.05),
];

/// One spark in flight — what `expect_sparks` counts.
#[derive(Component, Clone, Copy, Debug)]
pub struct Spark {
    /// Where it is, in canvas pixels, between whole ones.
    at: Vec2,
    velocity: Vec2,
    age: f32,
    flight: f32,
    depth: f32,
}

impl Spark {
    /// How far through its flight it is, 0 to 1.
    fn progress(&self) -> f32 {
        (self.age / self.flight).clamp(0.0, 1.0)
    }

    /// Its colour that far through: cooling, and fading over the last half.
    fn colour(&self) -> Color {
        let t = self.progress() * (COOLING.len() - 1) as f32;
        let from = (t as usize).min(COOLING.len() - 2);
        let colour = COOLING[from].mix(&COOLING[from + 1], t - from as f32);
        colour.with_alpha((2.0 - 2.0 * self.progress()).min(1.0))
    }

    /// Its size in whole canvas pixels: a texel, then two, then one.
    fn size(&self) -> f32 {
        match self.progress() {
            p if p < 0.4 => 3.0,
            p if p < 0.75 => 2.0,
            _ => 1.0,
        }
    }
}

/// Each talker's time to its next spark, and the frame it was last seen
/// talking, so somebody who stopped is forgotten; and the sparks' own dice.
#[derive(Resource)]
struct Sparks {
    due: HashMap<Uid, (f32, u32)>,
    frame: u32,
    flying: usize,
    rng: SmallRng,
}

impl Default for Sparks {
    fn default() -> Sparks {
        Sparks { due: HashMap::default(), frame: 0, flying: 0, rng: SmallRng::seed_from_u64(0x5ba4) }
    }
}

pub struct SmalltalkPlugin;

impl Plugin for SmalltalkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sparks>()
            .add_systems(OnExit(AppState::Game), forget_the_sparks)
            .add_systems(
                Update,
                (throw_sparks, fly_sparks)
                    .chain()
                    .in_set(super::SpriteSync)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            );
    }
}

/// The sparks go by `DespawnOnExit`; what is left is the count of them.
fn forget_the_sparks(mut sparks: ResMut<Sparks>) {
    sparks.due.clear();
    sparks.flying = 0;
}

/// A spark from everybody on the canvas making small talk whose turn it is.
fn throw_sparks(mut commands: Commands, time: Res<Time>, sim: Res<Sim>, crowd: Res<VisibleCrowd>, mut sparks: ResMut<Sparks>) {
    let state = &sim.0;
    let dt = time.delta_secs();
    sparks.frame = sparks.frame.wrapping_add(1);
    let frame = sparks.frame;
    for &slot in crowd.slots() {
        let Some(entity) = state.entities().slot(slot) else {
            continue;
        };
        let Some(smalltalk) = entity.smalltalk().filter(|smalltalk| smalltalk.answered) else {
            continue;
        };
        let Some(other) = state.entities().get(smalltalk.with) else {
            continue;
        };
        let from = world_pos(entity.position()) + Vec2::new(0.0, MOUTH);
        let to = world_pos(other.position()) + Vec2::new(0.0, MOUTH);
        // The first spark waits a turn of its own, so two who start at once
        // do not throw in step.
        let first = sparks.rng.random_range(0.0..SPARK_EVERY);
        let due = sparks.due.entry(entity.uid()).or_insert((first, frame));
        due.1 = frame;
        due.0 -= dt;
        let mut throws = 0;
        while due.0 <= 0.0 {
            due.0 += SPARK_EVERY;
            throws += 1;
        }
        for _ in 0..throws {
            if sparks.flying >= MOST_SPARKS {
                break;
            }
            let flight = sparks.rng.random_range(SHORTEST_FLIGHT..LONGEST_FLIGHT);
            // An arc that lands on the other's mouth at the end of the
            // flight, give or take a stray.
            let stray = Vec2::new(sparks.rng.random_range(-STRAY..STRAY), sparks.rng.random_range(0.0..STRAY));
            let velocity = (to - from) / flight + Vec2::new(0.0, GRAVITY * flight / 2.0) + stray;
            let spark = Spark { at: from, velocity, age: 0.0, flight, depth: depth_for(from.y) + SPARK_DEPTH };
            commands.spawn((
                Name::new("spark"),
                Sprite::from_color(spark.colour(), Vec2::splat(spark.size())),
                Anchor::BOTTOM_LEFT,
                Transform::from_translation(from.round().extend(spark.depth)),
                WORLD_LAYER,
                DespawnOnExit(AppState::Game),
                spark,
            ));
            sparks.flying += 1;
        }
    }
    // Anybody who stopped talking, or went off the canvas, starts afresh.
    sparks.due.retain(|_, &mut (_, seen)| seen == frame);
}

/// Every spark on along its arc, cooling; gone at the end of its flight.
fn fly_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut sparks: ResMut<Sparks>,
    mut flying: Query<(Entity, &mut Spark, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (id, mut spark, mut transform, mut sprite) in &mut flying {
        spark.age += dt;
        if spark.age >= spark.flight {
            commands.entity(id).despawn();
            sparks.flying = sparks.flying.saturating_sub(1);
            continue;
        }
        spark.velocity.y -= GRAVITY * dt;
        let step = spark.velocity * dt;
        spark.at += step;
        transform.translation = spark.at.round().extend(spark.depth);
        sprite.color = spark.colour();
        sprite.custom_size = Some(Vec2::splat(spark.size()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spark(age: f32) -> Spark {
        Spark { at: Vec2::ZERO, velocity: Vec2::ZERO, age, flight: 1.0, depth: 0.0 }
    }

    #[test]
    fn a_spark_cools_shrinks_and_fades_as_it_flies() {
        let (hot, cold) = (spark(0.0), spark(0.99));
        assert_eq!(hot.colour(), COOLING[0]);
        assert_eq!((hot.size(), spark(0.5).size(), cold.size()), (3.0, 2.0, 1.0));
        assert_eq!(hot.colour().alpha(), 1.0);
        assert!(cold.colour().alpha() < 0.05);
        // Red by the end, not white.
        let LinearRgba { red, blue, .. } = cold.colour().to_linear();
        assert!(red > blue * 4.0, "{:?}", cold.colour());
    }

    #[test]
    fn a_spark_thrown_at_somebody_lands_on_them_with_no_stray() {
        // The arc `throw_sparks` aims, flown in small steps as `fly_sparks`
        // would.
        let (from, to, flight) = (Vec2::new(0.0, 0.0), Vec2::new(48.0, 0.0), 0.7);
        let mut spark = Spark {
            at: from,
            velocity: (to - from) / flight + Vec2::new(0.0, GRAVITY * flight / 2.0),
            age: 0.0,
            flight,
            depth: 0.0,
        };
        let dt = 1.0 / 1000.0;
        while spark.age + dt <= flight {
            spark.age += dt;
            spark.velocity.y -= GRAVITY * dt;
            let step = spark.velocity * dt;
            spark.at += step;
        }
        assert!(spark.at.distance(to) < 1.5, "landed at {}", spark.at);
    }
}
