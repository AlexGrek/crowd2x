//! What an item looks like while somebody is holding it.
//!
//! Every [`ItemKind`] has to be drawable, and there are two ways to draw one: a
//! pixel-art texture or an emoji. [`art`] is a `match` over every kind with no
//! fallback arm, so a new item does not compile until somebody has said which —
//! an item that is never drawn is not a case to remember to check for.
//!
//! The simulation does not know any of this. An item says what it is made of;
//! which picture that is belongs here, next to the rest of the art, the same
//! way `sim` hands out an appearance seed and [`super::human`] decides which
//! PNGs it means. `game::held` is what puts the picture on the screen.

use bevy::prelude::*;

use super::ART_SCALE;
use crate::sim::ItemKind;

/// The picture of one kind of item.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemArt {
    /// Pixel art: a path relative to `assets/`, of any size.
    ///
    /// Stored at its true resolution and drawn at [`ART_SCALE`], like every
    /// other sprite, so that its pixels are the size of the pixels of whoever
    /// holds it — a picture at any other scale has finer or coarser pixels
    /// than the hand it is in. That makes the size of the file the size of the
    /// item: a 16x16 one is as big as a person, so an item's picture is drawn
    /// small. Whatever it is, [`centring_nudge`] keeps it centred on whole
    /// pixels.
    #[expect(dead_code, reason = "no item is drawn from a texture yet; this stops being expected the day one is")]
    Texture(&'static str),
    /// One emoji, drawn as text in the bundled colour-emoji font.
    ///
    /// A single codepoint, for the reason [`GoalId::emoji`] is: a sequence
    /// that a renderer does not join comes out as its parts.
    ///
    /// [`GoalId::emoji`]: crate::sim::GoalId::emoji
    Emoji(&'static str),
}

/// How an item is drawn.
///
/// Every kind, and no wildcard arm: that is the guarantee.
pub const fn art(kind: ItemKind) -> ItemArt {
    match kind {
        ItemKind::Food => ItemArt::Emoji("🍔"),
        ItemKind::Water => ItemArt::Emoji("💧"),
    }
}

/// How far to nudge a texture of this many texels, in canvas pixels, so that
/// centring it on a point on the pixel grid leaves its edges on the grid too.
///
/// A sprite is centred on its position, so its edges are half its size either
/// side of it. When that size is an even number of canvas pixels the edges
/// land on whole ones, as they do for everything else in the world. When it
/// is odd — a 5-texel texture is fifteen pixels — they land halfway between
/// two, neighbouring texels get different widths, and the picture shivers as
/// its carrier crosses a cell: the one thing the pipeline exists to prevent.
/// So an odd axis is moved half a pixel, which puts its middle on the middle
/// of a pixel instead of on the line between two, and is the closest a sprite
/// of that size can be centred on the grid.
///
/// The texture's size is only known once it has loaded, and the size that
/// counts is the source's: [`ART_SCALE`] is what makes it pixels.
pub fn centring_nudge(texels: UVec2) -> Vec2 {
    let pixels = texels.as_vec2() * ART_SCALE;
    Vec2::new(pixels.x % 2.0, pixels.y % 2.0) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characters::png_size;

    #[test]
    fn every_kind_of_item_can_be_drawn() {
        // The match already says so at compile time. What it cannot say is
        // whether the art it names is real.
        for kind in ItemKind::ALL {
            match art(kind) {
                ItemArt::Emoji(glyph) => assert_eq!(
                    glyph.chars().count(),
                    1,
                    "{} is drawn as {glyph:?}, which is not one codepoint",
                    kind.name()
                ),
                ItemArt::Texture(path) => {
                    let (width, height) = png_size(&format!("assets/{path}"));
                    assert!(width > 0 && height > 0, "{path} is empty");
                }
            }
        }
    }

    #[test]
    fn an_even_number_of_pixels_needs_no_nudge_and_an_odd_number_half_a_pixel() {
        // 16 texels is 48 pixels and 6 is 18: edges already on the grid.
        assert_eq!(centring_nudge(UVec2::new(16, 6)), Vec2::ZERO);
        // 5 texels is 15 pixels and 7 is 21: an odd number, per axis.
        assert_eq!(centring_nudge(UVec2::new(5, 6)), Vec2::new(0.5, 0.0));
        assert_eq!(centring_nudge(UVec2::new(6, 7)), Vec2::new(0.0, 0.5));
        assert_eq!(centring_nudge(UVec2::new(5, 7)), Vec2::splat(0.5));
    }

    #[test]
    fn whatever_the_size_the_edges_of_a_nudged_texture_are_on_whole_pixels() {
        // The property the nudge exists for, over every size an item could
        // reasonably be — rather than the arithmetic it is done with.
        for width in 1..=32u32 {
            for height in 1..=32u32 {
                let texels = UVec2::new(width, height);
                let pixels = texels.as_vec2() * ART_SCALE;
                let nudge = centring_nudge(texels);
                // Centred on a whole pixel, then nudged: where the edges are.
                for (edge, name) in [
                    (nudge.x - pixels.x / 2.0, "left"),
                    (nudge.x + pixels.x / 2.0, "right"),
                    (nudge.y - pixels.y / 2.0, "bottom"),
                    (nudge.y + pixels.y / 2.0, "top"),
                ] {
                    assert_eq!(
                        edge.fract(),
                        0.0,
                        "{width}x{height}: the {name} edge is at {edge}, between two pixels"
                    );
                }
            }
        }
    }
}
