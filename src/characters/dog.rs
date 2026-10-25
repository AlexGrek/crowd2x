//! Dogs, and deathdogs: one 4-frame idle animation each, with a separate
//! mirrored sheet for left-facing ones rather than `flip_x`, so the
//! left-facing art can be hand-tuned independently later.
//!
//! Every sheet is a strip of 16x16 frames, upscaled to a cell when drawn. A
//! deathdog is a dog's body in other colours (`tools/deathdog_art.py`), so the
//! two share one layout, one pool of bodies and one way of being drawn; which
//! sheets a body is drawn from is its [`Breed`].

use bevy::prelude::*;

use super::{depth_for, snap_to_texel, upscale, Character, ART, ART_SCALE};
use crate::animation::FrameAnimation;
use crate::render::WORLD_LAYER;

pub(super) const IDLE_RIGHT: &str = "dog_idle.png";
pub(super) const IDLE_LEFT: &str = "dog_idle_reversed.png";
pub(super) const DEATHDOG_IDLE_RIGHT: &str = "deathdog_idle.png";
pub(super) const DEATHDOG_IDLE_LEFT: &str = "deathdog_idle_reversed.png";
pub(super) const IDLE_FRAMES: u32 = 4;
const SECONDS_PER_FRAME: f32 = 0.1;

/// Which animal a dog's body is drawn as.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breed {
    Dog,
    Deathdog,
}

/// One breed's two idle sheets.
#[derive(Clone)]
struct Sheets {
    right: Handle<Image>,
    left: Handle<Image>,
}

/// Every breed's idle sheets plus the atlas layout they share.
#[derive(Resource, Clone)]
pub struct DogSheets {
    dog: Sheets,
    deathdog: Sheets,
    layout: Handle<TextureAtlasLayout>,
}

impl DogSheets {
    pub fn load(assets: &AssetServer, layouts: &mut Assets<TextureAtlasLayout>) -> Self {
        Self {
            dog: Sheets { right: assets.load(IDLE_RIGHT), left: assets.load(IDLE_LEFT) },
            deathdog: Sheets { right: assets.load(DEATHDOG_IDLE_RIGHT), left: assets.load(DEATHDOG_IDLE_LEFT) },
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

    /// The first idle frame, for drawing one outside the world — the unit
    /// panel's portrait. The atlas comes with it, because one frame of a strip
    /// is a region of a sheet and not an image of its own.
    pub fn portrait(&self, breed: Breed, facing: Facing) -> (Handle<Image>, TextureAtlas) {
        (
            self.image_for(breed, facing),
            TextureAtlas {
                layout: self.layout.clone(),
                index: 0,
            },
        )
    }

    pub(crate) fn image_for(&self, breed: Breed, facing: Facing) -> Handle<Image> {
        let sheets = match breed {
            Breed::Dog => &self.dog,
            Breed::Deathdog => &self.deathdog,
        };
        match facing {
            Facing::Right => sheets.right.clone(),
            Facing::Left => sheets.left.clone(),
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Left,
    Right,
}

/// Returns the entity it made, so a caller can keep track of it.
pub fn spawn(commands: &mut Commands, sheets: &DogSheets, pos: Vec2, breed: Breed, facing: Facing) -> Entity {
    // See `human::spawn`: positions live on the art's own pixel grid.
    let pos = snap_to_texel(pos);
    commands
        .spawn((
            Name::new(match breed {
                Breed::Dog => "dog",
                Breed::Deathdog => "deathdog",
            }),
            Character,
            breed,
            facing,
            Sprite::from_atlas_image(
                sheets.image_for(breed, facing),
                TextureAtlas {
                    layout: sheets.layout.clone(),
                    index: 0,
                },
            ),
            FrameAnimation::new(0, IDLE_FRAMES as usize - 1, SECONDS_PER_FRAME),
            Transform::from_xyz(pos.x, pos.y, depth_for(pos.y)).with_scale(upscale(ART_SCALE)),
            WORLD_LAYER,
        ))
        .id()
}

/// Point an existing body at another dog, of either breed — the pool's half
/// of [`spawn`].
///
/// One entity, so there are no children to redress; what has to be reset is
/// the breed, the sheet, the atlas index and the animation timer, or a
/// recycled dog picks up mid-stride, or in the colours of, whoever had the
/// body last. Position first and visibility last, for the reason
/// [`super::human::redress`] gives.
pub fn redress(
    commands: &mut Commands,
    sheets: &DogSheets,
    root: Entity,
    pos: Vec2,
    breed: Breed,
    facing: Facing,
) {
    let pos = snap_to_texel(pos);
    commands.entity(root).insert((
        breed,
        facing,
        Sprite::from_atlas_image(
            sheets.image_for(breed, facing),
            TextureAtlas {
                layout: sheets.layout.clone(),
                index: 0,
            },
        ),
        FrameAnimation::new(0, IDLE_FRAMES as usize - 1, SECONDS_PER_FRAME),
        Transform::from_xyz(pos.x, pos.y, depth_for(pos.y)).with_scale(upscale(ART_SCALE)),
        Visibility::Inherited,
    ));
}

/// Swap to the matching sheet when a dog turns around.
pub fn apply_facing(
    sheets: Option<Res<DogSheets>>,
    mut dogs: Query<(&Breed, &Facing, &mut Sprite), Changed<Facing>>,
) {
    let Some(sheets) = sheets else {
        return;
    };
    for (breed, facing, mut sprite) in &mut dogs {
        sprite.image = sheets.image_for(*breed, *facing);
    }
}
