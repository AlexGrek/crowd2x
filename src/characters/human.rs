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

use super::{depth_for, snap_to_texel, upscale, Character, ART_SCALE};
use crate::render::WORLD_LAYER;

pub const BASE: &str = "human/human_base.png";
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

/// How many layers are drawn over the base body.
pub const SLOTS: usize = 3;

/// The layer slots over the base body, in draw order. `None` is a slot this
/// look does not wear.
///
/// A fixed array and not a list of what is worn, which is what makes a body
/// **recyclable**: 15% of looks have no outfit, and if that meant a human were
/// three entities instead of four, one could not be dressed as another without
/// being rebuilt. An empty slot is a hidden child, so every paperdoll in the
/// world is the same four entities in the same order.
fn slot_paths(look: &Look) -> [Option<&'static str>; SLOTS] {
    [Some(look.eyes), look.clothes, Some(look.hair)]
}

/// Depth offsets per slot, in the same order.
const SLOT_DEPTHS: [f32; SLOTS] = [LAYER_EYES, LAYER_CLOTHES, LAYER_HAIR];

/// The layer entities of one paperdoll, in slot order.
///
/// Held on the root so redressing addresses a slot directly rather than
/// trusting `Children` order, which nothing promises to preserve.
#[derive(Component)]
pub struct Paperdoll {
    pub layers: [Entity; SLOTS],
}

/// Every layer of a paperdoll, base first, in draw order.
///
/// For drawing the same person somewhere that is not the world — the unit
/// panel's portrait stacks these as UI nodes. It comes from here, off the same
/// [`Look`], so a portrait cannot show an outfit its character is not wearing.
/// An empty slot is skipped, so a portrait is still only what is worn.
pub fn portrait_layers(look: &Look) -> Vec<&'static str> {
    std::iter::once(BASE)
        .chain(slot_paths(look).into_iter().flatten())
        .collect()
}

/// Returns the root entity: the paperdoll's layers are its children, so this
/// is the one to move, despawn, or hang a marker on.
pub fn spawn(commands: &mut Commands, assets: &AssetServer, pos: Vec2, look: Look) -> Entity {
    let paths = slot_paths(&look);

    // On the art's own grid: see `characters::snap_to_texel`.
    let pos = snap_to_texel(pos);

    // Spawned separately rather than through `with_children` so their ids can
    // go on `Paperdoll`.
    let layers = std::array::from_fn(|slot| {
        commands
            .spawn((
                LookLayer,
                match paths[slot] {
                    Some(path) => Sprite::from_image(assets.load(path)),
                    // An empty slot still exists, hidden — see `slot_paths`.
                    None => Sprite::default(),
                },
                visibility_for(paths[slot]),
                Transform::from_xyz(0.0, 0.0, SLOT_DEPTHS[slot]),
                WORLD_LAYER,
            ))
            .id()
    });

    let root = commands
        .spawn((
            Name::new("human"),
            Character,
            look,
            Paperdoll { layers },
            Sprite::from_image(assets.load(BASE)),
            Transform::from_xyz(pos.x, pos.y, depth_for(pos.y))
                .with_scale(upscale(ART_SCALE)),
            WORLD_LAYER,
        ))
        .id();
    commands.entity(root).add_children(&layers);
    root
}

fn visibility_for(path: Option<&'static str>) -> Visibility {
    if path.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Dress an existing body as somebody else — the pool's half of [`spawn`].
///
/// Everything goes through `Commands` rather than through queries. Redressing
/// happens only when a unit crosses the edge of the view, a handful of times a
/// second, so it does not need to be fast; it needs to be obviously correct,
/// and an `insert` over a component that is already there is exactly that.
///
/// The position is written here, **before** the body is made visible, so a
/// recycled body never draws a frame where its previous tenant stood.
pub fn redress(
    commands: &mut Commands,
    assets: &AssetServer,
    root: Entity,
    doll: &Paperdoll,
    pos: Vec2,
    look: Look,
) {
    let paths = slot_paths(&look);
    for slot in 0..SLOTS {
        let mut layer = commands.entity(doll.layers[slot]);
        match paths[slot] {
            Some(path) => {
                layer.insert((Sprite::from_image(assets.load(path)), Visibility::Inherited));
            }
            None => {
                layer.insert(Visibility::Hidden);
            }
        }
    }

    let pos = snap_to_texel(pos);
    commands.entity(root).insert((
        look,
        Transform::from_xyz(pos.x, pos.y, depth_for(pos.y)).with_scale(upscale(ART_SCALE)),
        Visibility::Inherited,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    /// Every human is the same number of layer slots, dressed or not. This is
    /// what makes one body recyclable into another: a look with no outfit has
    /// an empty slot, never a missing one.
    #[test]
    fn every_human_has_the_same_three_layer_slots() {
        let dressed = Look {
            clothes: Some(CLOTHES[0]),
            eyes: EYES[0],
            hair: HAIR[0],
        };
        let bare = Look {
            clothes: None,
            ..dressed.clone()
        };
        assert_eq!(slot_paths(&dressed).len(), SLOTS);
        assert_eq!(slot_paths(&bare).len(), SLOTS);
        assert_eq!(slot_paths(&bare)[1], None);
        assert_eq!(slot_paths(&dressed)[1], Some(CLOTHES[0]));
    }

    /// The portrait shows what is worn and nothing else, so the refactor to
    /// fixed slots cannot silently put an empty layer in the unit panel.
    #[test]
    fn a_portrait_still_skips_a_missing_outfit() {
        let dressed = Look {
            clothes: Some(CLOTHES[1]),
            eyes: EYES[0],
            hair: HAIR[2],
        };
        assert_eq!(
            portrait_layers(&dressed),
            vec![BASE, EYES[0], CLOTHES[1], HAIR[2]]
        );

        let bare = Look {
            clothes: None,
            ..dressed
        };
        assert_eq!(portrait_layers(&bare), vec![BASE, EYES[0], HAIR[2]]);
    }

    /// The slot depths keep the draw order the art was made for, and stay
    /// inside the band `depth_for` leaves for one character.
    #[test]
    fn the_slots_are_in_draw_order_over_the_body() {
        assert!(SLOT_DEPTHS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(SLOT_DEPTHS[0] > 0.0);
        // One world pixel of Y is 0.01 of depth: a layer must never reach into
        // the slot of the character standing in front.
        assert!(SLOT_DEPTHS[SLOTS - 1] < 0.01);
    }

    /// A seeded look is the same person every time — what lets a unit leave
    /// the view and come back wearing what it was wearing.
    #[test]
    fn the_same_seed_is_the_same_person() {
        let of = |seed| Look::random(&mut SmallRng::seed_from_u64(seed));
        assert_eq!(portrait_layers(&of(7)), portrait_layers(&of(7)));
    }
}
