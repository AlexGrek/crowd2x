//! Layered paperdoll humans.
//!
//! Every layer is a full 16x16 frame drawn at the same position, so a character
//! is just a stack of sprites in a fixed order. Adding a new hairstyle or outfit
//! means dropping a PNG into `assets/human/` and listing it here.
//!
//! The stack is scaled once, on the root: the children inherit it, so a layer
//! can never end up at a different scale from the body it is drawn on.

use bevy::prelude::*;
use rand::prelude::*;

use super::{depth_for, upscale, Character, ART_SCALE};
use crate::render::WORLD_LAYER;

pub(super) const BASE: &str = "human/human_base.png";
pub(super) const CLOTHES: [&str; 2] = [
    "human/clothes_swimsuit_blue.png",
    "human/clothes_swimsuit_pink.png",
];
pub(super) const EYES: [&str; 1] = ["human/eyes_brown.png"];
pub(super) const HAIR: [&str; 3] = [
    "human/hair_blonde.png",
    "human/hair_blonde_longer.png",
    "human/hair_blue.png",
];

/// Depth offsets within a single character, applied on top of `depth_for`.
/// Ordering matches the source art: eyes sit on the face, clothes over the
/// body, hair over both.
const LAYER_EYES: f32 = 0.001;
const LAYER_CLOTHES: f32 = 0.002;
const LAYER_HAIR: f32 = 0.003;

/// The randomised appearance of one human.
#[derive(Component, Debug, Clone)]
pub struct Look {
    pub clothes: Option<&'static str>,
    pub eyes: &'static str,
    pub hair: &'static str,
}

impl Look {
    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        Self {
            // A small chance of no outfit at all, so the base body is visible.
            clothes: if rng.random_range(0..100) < 85 {
                CLOTHES.choose(rng).copied()
            } else {
                None
            },
            eyes: EYES.choose(rng).copied().expect("no eye assets listed"),
            hair: HAIR.choose(rng).copied().expect("no hair assets listed"),
        }
    }
}

/// Marks one sprite layer of a paperdoll character.
#[derive(Component)]
pub struct LookLayer;

pub fn spawn(commands: &mut Commands, assets: &AssetServer, pos: Vec2, look: Look) {
    let layers: Vec<(&'static str, f32)> = [
        Some((look.eyes, LAYER_EYES)),
        look.clothes.map(|c| (c, LAYER_CLOTHES)),
        Some((look.hair, LAYER_HAIR)),
    ]
    .into_iter()
    .flatten()
    .collect();

    // Whole pixels only: on a fractional coordinate the upscaled texels come
    // out uneven, some three canvas pixels wide and some four.
    let pos = pos.round();

    commands
        .spawn((
            Name::new("human"),
            Character,
            look,
            Sprite::from_image(assets.load(BASE)),
            Transform::from_xyz(pos.x, pos.y, depth_for(pos.y))
                .with_scale(upscale(ART_SCALE)),
            WORLD_LAYER,
        ))
        .with_children(|parent| {
            for (path, z) in layers {
                parent.spawn((
                    LookLayer,
                    Sprite::from_image(assets.load(path)),
                    Transform::from_xyz(0.0, 0.0, z),
                    WORLD_LAYER,
                ));
            }
        });
}
