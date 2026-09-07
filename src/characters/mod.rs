//! Character rendering: the only thing drawn into the world canvas for now.
//!
//! Two kinds exist, both on a 48x48 sprite grid:
//!   * humans - a layered paperdoll (body, clothes, eyes, hair)
//!   * dogs   - a single 4-frame idle animation with a mirrored left-facing sheet

pub mod dog;
pub mod human;

use bevy::prelude::*;

/// Every character occupies a 48x48 cell of the canvas.
pub const CELL: u32 = 48;

/// ...but the art is *drawn* at 16x16 and upscaled to fill that cell.
///
/// Source PNGs are stored at their true resolution, never pre-upscaled: an
/// upscaled file is the same picture with the scale baked in where the game
/// can no longer choose it, and it hides which art is genuinely detailed.
/// `tools/art_scale.py` finds files that have drifted back to an upscale.
pub const ART: u32 = 16;

/// Whole-number upscale from a source texel to canvas pixels.
///
/// It has to stay an integer. At a fractional scale a texel covers a
/// non-whole number of canvas pixels, nearest sampling gives neighbouring
/// texels different widths, and the pixel grid the whole renderer exists to
/// protect breaks inside the sprite.
pub const ART_SCALE: f32 = (CELL / ART) as f32;

/// Sprite scale for art drawn at [`ART`] resolution.
///
/// Z is deliberately left at 1: it carries painter's-order depth, and scaling
/// the per-layer offsets in [`human`] would push a character's hair into the
/// depth slot of the character standing in front of it.
pub fn upscale(factor: f32) -> Vec3 {
    Vec3::new(factor, factor, 1.0)
}

/// Pixel size of a PNG, read straight out of its IHDR.
///
/// Only the tests need this, and only to check that a file on disk is the
/// resolution the code claims — not worth an image dependency.
#[cfg(test)]
pub(crate) fn png_size(path: &str) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let field = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    // 8 byte signature, then the IHDR length and type, then width and height.
    (field(16), field(20))
}

/// Marks the root entity of a character (its sprite layers are children).
#[derive(Component)]
pub struct Character;

/// Painter's-order depth from a world position: characters lower on the screen
/// are drawn in front. The step per pixel is large enough that a character can
/// never be interleaved with another character's layers.
pub fn depth_for(y: f32) -> f32 {
    -y * 0.01
}

/// The art side of a character: how one is built, scaled and depth-sorted.
///
/// It spawns nothing of its own. Who exists is [`crate::sim`]'s answer, and
/// `game::actors` is what turns that answer into sprites — this module used to
/// scatter a demo crowd on `Startup`, which put twenty-eight characters
/// off the edge of every map, in every screen, for the life of the process.
pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, dog::apply_facing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Art is stored at its true resolution and upscaled by the game, so a
    /// file that arrives pre-upscaled would silently draw at three times its
    /// intended size. `tools/art_scale.py` shrinks one back down.
    #[test]
    fn character_art_is_drawn_at_the_art_resolution() {
        let layers = std::iter::once(human::BASE)
            .chain(human::CLOTHES)
            .chain(human::EYES)
            .chain(human::HAIR);
        for path in layers {
            assert_eq!(
                png_size(&format!("assets/{path}")),
                (ART, ART),
                "{path} is not {ART}x{ART}"
            );
        }
    }

    #[test]
    fn dog_sheets_are_strips_of_art_sized_frames() {
        for path in [dog::IDLE_RIGHT, dog::IDLE_LEFT] {
            assert_eq!(
                png_size(&format!("assets/{path}")),
                (ART * dog::IDLE_FRAMES, ART),
                "{path} does not hold {} frames of {ART}x{ART}",
                dog::IDLE_FRAMES
            );
        }
    }

    #[test]
    fn upscaling_leaves_depth_alone() {
        // Scaling z as well would multiply the paperdoll's layer offsets and
        // let a character's hair cross into the next character's depth range.
        assert_eq!(upscale(ART_SCALE).z, 1.0);
    }
}
