//! Sleep, drawn: zzz rising from everybody lying asleep.
//!
//! That somebody is asleep is the simulation's ([`GameEntity::is_asleep`]);
//! `actors` turns the doll on its side for it, as it does the dead, and this
//! lets a "Z" float up from its head every [`Z_EVERY`] seconds — a letter of
//! five texels by five, drawn small and growing, swaying a little to one side
//! as it climbs and fading out — so a sleeper is told from the dead at a
//! glance and a bedroom at night is a drift of them.
//!
//! **Real time**, [`Time`] and not the fixed step, like the sparks and the
//! goal emoji's fade: a Z is a reading, not a fact about the world, and it
//! keeps rising while the game is paused. Each Z is one sprite of a texture
//! made here ([`z_pixels`]), placed on a whole canvas pixel by its corner
//! ([`Anchor::BOTTOM_LEFT`]) and sized in whole texels of two or three canvas
//! pixels, so none ever lands between two. There are never more than
//! [`MOST_ZS`] at once, whatever the crowd.
//!
//! [`GameEntity::is_asleep`]: crate::sim::GameEntity::is_asleep

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::Anchor;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

use crate::characters::depth_for;
use crate::render::WORLD_LAYER;
use crate::sim::Uid;
use crate::state::AppState;

use super::actors::{world_pos, Sim, VisibleCrowd};

/// Seconds between one sleeper's Zs.
const Z_EVERY: f32 = 0.9;

/// How long a Z floats, in seconds.
const LIFE: f32 = 2.4;

/// Canvas pixels a second a Z climbs.
const RISE: f32 = 14.0;

/// How far a Z sways to either side, in canvas pixels, and how fast.
const SWAY: f32 = 3.0;
const SWAY_RATE: f32 = 3.5;

/// Canvas pixels a second a Z drifts to the right.
const DRIFT: f32 = 4.0;

/// Where a Z leaves from, over the middle of a sleeper's cell, in canvas
/// pixels: its head, which lying on its side is to the left of the middle.
const HEAD: Vec2 = Vec2::new(-16.0, 6.0);

/// The most Zs afloat at once.
const MOST_ZS: usize = 200;

/// In front of everything about a character, with the sparks: inside the 0.01
/// [`depth_for`] leaves between one pixel of Y and the next.
const Z_DEPTH: f32 = 0.008;

/// The letter's side in texels.
const GLYPH: usize = 5;

/// A Z's colour: a pale blue, as sleep is.
const PALE: Color = Color::srgb(0.78, 0.88, 1.0);

/// One Z afloat.
#[derive(Component, Clone, Copy, Debug)]
pub struct Zzz {
    /// Where it started, in canvas pixels.
    from: Vec2,
    age: f32,
    /// Where in its sway it starts, so a row of sleepers do not sway in step.
    phase: f32,
    depth: f32,
}

impl Zzz {
    /// How far through its life it is, 0 to 1.
    fn progress(&self) -> f32 {
        (self.age / LIFE).clamp(0.0, 1.0)
    }

    /// Where it is now, between whole pixels: up, a little to the right, and
    /// swaying.
    fn at(&self) -> Vec2 {
        self.from + Vec2::new(self.age * DRIFT + SWAY * (self.age * SWAY_RATE + self.phase).sin(), self.age * RISE)
    }

    /// Its size in canvas pixels: a texel of two to begin with, then three.
    fn size(&self) -> f32 {
        let texel = if self.progress() < 0.35 { 2.0 } else { 3.0 };
        texel * GLYPH as f32
    }

    /// Solid for the first half of its life, then fading out.
    fn colour(&self) -> Color {
        PALE.with_alpha((2.0 - 2.0 * self.progress()).min(1.0))
    }
}

/// The picture of a Z, made once.
#[derive(Resource)]
struct ZImage(Handle<Image>);

/// Each sleeper's time to its next Z, and the frame it was last seen asleep,
/// so somebody who woke is forgotten; and the Zs' own dice.
#[derive(Resource)]
struct Sleepers {
    due: HashMap<Uid, (f32, u32)>,
    frame: u32,
    afloat: usize,
    rng: SmallRng,
}

impl Default for Sleepers {
    fn default() -> Sleepers {
        Sleepers { due: HashMap::default(), frame: 0, afloat: 0, rng: SmallRng::seed_from_u64(0x2a2a) }
    }
}

pub struct SnoozePlugin;

impl Plugin for SnoozePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sleepers>()
            .add_systems(Startup, make_the_z)
            .add_systems(OnExit(AppState::Game), forget_the_sleepers)
            .add_systems(
                Update,
                (let_zs_rise, float_zs)
                    .chain()
                    .in_set(super::SpriteSync)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            );
    }
}

/// The Z as pixels: white where the letter is, clear elsewhere, so a sprite
/// tints it and fades it.
///
/// ```text
/// #####
/// ...#.
/// ..#..
/// .#...
/// #####
/// ```
fn z_pixels() -> [bool; GLYPH * GLYPH] {
    const Z: [&str; GLYPH] = ["#####", "...#.", "..#..", ".#...", "#####"];
    let mut pixels = [false; GLYPH * GLYPH];
    for (y, row) in Z.iter().enumerate() {
        for (x, texel) in row.bytes().enumerate() {
            pixels[y * GLYPH + x] = texel == b'#';
        }
    }
    pixels
}

fn make_the_z(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let data: Vec<u8> = z_pixels().iter().flat_map(|&set| if set { [255, 255, 255, 255] } else { [255, 255, 255, 0] }).collect();
    let image = Image::new(
        Extent3d { width: GLYPH as u32, height: GLYPH as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    commands.insert_resource(ZImage(images.add(image)));
}

/// The Zs go by `DespawnOnExit`; what is left is the count of them.
fn forget_the_sleepers(mut sleepers: ResMut<Sleepers>) {
    sleepers.due.clear();
    sleepers.afloat = 0;
}

/// A Z from everybody asleep on the canvas whose turn it is.
fn let_zs_rise(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    crowd: Res<VisibleCrowd>,
    z: Res<ZImage>,
    mut sleepers: ResMut<Sleepers>,
) {
    let state = &sim.0;
    let dt = time.delta_secs();
    sleepers.frame = sleepers.frame.wrapping_add(1);
    let frame = sleepers.frame;
    for &slot in crowd.slots() {
        let Some(entity) = state.entities().slot(slot) else {
            continue;
        };
        if !entity.is_asleep() {
            continue;
        }
        let from = world_pos(entity.position()) + HEAD;
        // The first Z waits a turn of its own, so a room that fell asleep
        // together does not breathe in step.
        let first = sleepers.rng.random_range(0.0..Z_EVERY);
        let due = sleepers.due.entry(entity.uid()).or_insert((first, frame));
        due.1 = frame;
        due.0 -= dt;
        if due.0 > 0.0 {
            continue;
        }
        due.0 += Z_EVERY;
        if sleepers.afloat >= MOST_ZS {
            continue;
        }
        let phase = sleepers.rng.random_range(0.0..std::f32::consts::TAU);
        let zzz = Zzz { from, age: 0.0, phase, depth: depth_for(from.y) + Z_DEPTH };
        commands.spawn((
            Name::new("zzz"),
            Sprite { image: z.0.clone(), custom_size: Some(Vec2::splat(zzz.size())), color: zzz.colour(), ..default() },
            Anchor::BOTTOM_LEFT,
            Transform::from_translation(zzz.at().round().extend(zzz.depth)),
            WORLD_LAYER,
            DespawnOnExit(AppState::Game),
            zzz,
        ));
        sleepers.afloat += 1;
    }
    // Anybody who woke, or went off the canvas, starts afresh.
    sleepers.due.retain(|_, &mut (_, seen)| seen == frame);
}

/// Every Z a little higher, swaying, paler; gone at the end of its life.
fn float_zs(
    mut commands: Commands,
    time: Res<Time>,
    mut sleepers: ResMut<Sleepers>,
    mut afloat: Query<(Entity, &mut Zzz, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (id, mut zzz, mut transform, mut sprite) in &mut afloat {
        zzz.age += dt;
        if zzz.age >= LIFE {
            commands.entity(id).despawn();
            sleepers.afloat = sleepers.afloat.saturating_sub(1);
            continue;
        }
        transform.translation = zzz.at().round().extend(zzz.depth);
        sprite.color = zzz.colour();
        sprite.custom_size = Some(Vec2::splat(zzz.size()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zzz(age: f32) -> Zzz {
        Zzz { from: Vec2::ZERO, age, phase: 0.0, depth: 0.0 }
    }

    #[test]
    fn the_letter_is_a_z() {
        let pixels = z_pixels();
        let row = |y: usize| &pixels[y * GLYPH..(y + 1) * GLYPH];
        assert_eq!(row(0), [true; GLYPH], "a bar on top");
        assert_eq!(row(4), [true; GLYPH], "and one below");
        assert_eq!(row(1), [false, false, false, true, false]);
        assert_eq!(row(2), [false, false, true, false, false]);
        assert_eq!(row(3), [false, true, false, false, false], "a stroke between, top right to bottom left");
    }

    #[test]
    fn a_z_climbs_grows_and_fades_as_it_floats() {
        let (young, old) = (zzz(0.0), zzz(LIFE - 0.01));
        assert!(old.at().y > young.at().y + RISE, "{} -> {}", young.at().y, old.at().y);
        assert_eq!((young.size(), old.size()), (2.0 * GLYPH as f32, 3.0 * GLYPH as f32), "whole texels of two, then three");
        assert_eq!(young.colour().alpha(), 1.0);
        assert!(zzz(LIFE * 0.5).colour().alpha() >= 0.99, "solid for the first half");
        assert!(old.colour().alpha() < 0.05, "gone by the end");
    }

    #[test]
    fn a_z_sways_to_both_sides_of_its_climb() {
        let xs: Vec<f32> = (0..24).map(|i| zzz(i as f32 * 0.1).at().x - i as f32 * 0.1 * DRIFT).collect();
        assert!(xs.iter().any(|&x| x > SWAY * 0.5) && xs.iter().any(|&x| x < -SWAY * 0.5), "{xs:?}");
        assert!(xs.iter().all(|&x| x.abs() <= SWAY + 1e-4));
    }
}
