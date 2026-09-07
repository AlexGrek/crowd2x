//! Dogs: one 4-frame idle animation, with a separate mirrored sheet for
//! left-facing dogs rather than `flip_x`, so the left-facing art can be
//! hand-tuned independently later.
//!
//! Both sheets are strips of 16x16 frames, upscaled to a cell when drawn.

use bevy::prelude::*;
use rand::prelude::*;

use super::{depth_for, upscale, Character, ART, ART_SCALE};
use crate::animation::FrameAnimation;
use crate::render::WORLD_LAYER;

pub(super) const IDLE_RIGHT: &str = "dog_idle.png";
pub(super) const IDLE_LEFT: &str = "dog_idle_reversed.png";
pub(super) const IDLE_FRAMES: u32 = 4;
const SECONDS_PER_FRAME: f32 = 0.1;

/// Both idle sheets plus the atlas layout they share.
#[derive(Resource, Clone)]
pub struct DogSheets {
    right: Handle<Image>,
    left: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
}

impl DogSheets {
    pub fn load(assets: &AssetServer, layouts: &mut Assets<TextureAtlasLayout>) -> Self {
        Self {
            right: assets.load(IDLE_RIGHT),
            left: assets.load(IDLE_LEFT),
            layout: layouts.add(TextureAtlasLayout::from_grid(
                // Frames are indexed in the source art, not on screen.
                UVec2::splat(ART),
                IDLE_FRAMES,
                1,
                None,
                None,
            )),
        }
    }

    fn image_for(&self, facing: Facing) -> Handle<Image> {
        match facing {
            Facing::Right => self.right.clone(),
            Facing::Left => self.left.clone(),
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Left,
    Right,
}

impl Facing {
    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        if rng.random() {
            Facing::Left
        } else {
            Facing::Right
        }
    }
}

pub fn spawn(commands: &mut Commands, sheets: &DogSheets, pos: Vec2, facing: Facing) {
    // See `human::spawn`: fractional positions break the upscaled texel grid.
    let pos = pos.round();
    commands.spawn((
        Name::new("dog"),
        Character,
        facing,
        Sprite::from_atlas_image(
            sheets.image_for(facing),
            TextureAtlas {
                layout: sheets.layout.clone(),
                index: 0,
            },
        ),
        FrameAnimation::new(0, IDLE_FRAMES as usize - 1, SECONDS_PER_FRAME),
        Transform::from_xyz(pos.x, pos.y, depth_for(pos.y)).with_scale(upscale(ART_SCALE)),
        WORLD_LAYER,
    ));
}

/// Swap to the matching sheet when a dog turns around.
pub fn apply_facing(
    sheets: Option<Res<DogSheets>>,
    mut dogs: Query<(&Facing, &mut Sprite), Changed<Facing>>,
) {
    let Some(sheets) = sheets else {
        return;
    };
    for (facing, mut sprite) in &mut dogs {
        sprite.image = sheets.image_for(*facing);
    }
}
