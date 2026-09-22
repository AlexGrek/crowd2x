//! What somebody is holding, drawn in front of them.
//!
//! What an item looks like is [`characters::item::art`]'s answer — a small
//! pixel-art texture or an emoji — and this is what puts that picture in the
//! world: one sprite for anyone whose hand is full, in front of every layer of
//! their paperdoll, moving with them, and gone the frame the hand is empty.
//!
//! Only the **hand** is drawn. What is stowed is packed away in a pocket or a
//! bag, and a person with twelve meals put by looks like a person with none.
//! That is the distinction [`crate::sim::Inventory`] is built on, and it is the
//! one a viewer can see.
//!
//! Like the action bar and the goal label, the picture is a sprite of its own
//! rather than a child of the character's. A paperdoll's root carries the
//! 3x upscale, which would be inherited by an emoji — text is rasterised at the
//! size it is asked for and has no business being scaled on top of that.
//!
//! [`characters::item::art`]: crate::characters::item::art

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::characters::item::{self, ItemArt};
use crate::characters::{depth_for, upscale, ART_SCALE};
use crate::render::WORLD_LAYER;
use crate::sim::{ItemKind, Uid};
use crate::state::AppState;
use crate::view::VisibleArea;

use super::actors::{world_pos, Sim, VisibleCrowd, EMOJI_FONT};

/// Where the item sits relative to the middle of its carrier's cell, in canvas
/// pixels — whole ones, because the carrier is on the art's grid and the item
/// has to stay on the canvas's. Low and central: in front of the body, about
/// where the hands are.
const HELD_X: f32 = 0.0;
const HELD_Y: f32 = -6.0;

/// Canvas pixels tall an emoji is drawn at. Small enough to read as something
/// carried and not as something standing next to the carrier.
const HELD_EMOJI_SIZE: f32 = 14.0;

/// In front of a paperdoll's own layers (hair reaches 0.003) and of the
/// selection frame (0.004), behind the action bar (0.006) and the goal label
/// (0.007) — so a bar over a carrier's head is never hidden by what they hold,
/// and every offset stays well inside the 0.01 that [`depth_for`] leaves
/// between one pixel of Y and the next.
const HELD_DEPTH: f32 = 0.005;

/// Marks a sprite that is drawing an item somebody is holding, and says which.
///
/// What `expect_held` counts: the simulation saying a hand is full and the
/// screen showing it are different claims.
#[derive(Component, Clone, Copy)]
pub struct HeldItem(pub ItemKind);

/// Which sprite is drawing which carrier's item, and which item that is — so
/// a swap of one item for another is noticed without asking the sprite.
///
/// Spawned when a hand fills and despawned when it empties, rather than kept
/// around hidden: most of a crowd has empty hands at any moment, and a hidden
/// sprite still costs a place in every query that visits it.
#[derive(Resource, Default)]
struct HeldSprites {
    by_uid: HashMap<Uid, (Entity, ItemKind)>,
}

pub struct HeldPlugin;

impl Plugin for HeldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeldSprites>()
            .add_systems(OnExit(AppState::Game), forget_the_sprites)
            .add_systems(
                Update,
                sync_held_items
                    .in_set(super::SpriteSync)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            );
    }
}

/// The sprites go by `DespawnOnExit`; what is left is the bookkeeping that
/// would otherwise point at entities that no longer exist.
fn forget_the_sprites(mut held: ResMut<HeldSprites>) {
    held.by_uid.clear();
}

/// Make the pictures match the hands.
fn sync_held_items(
    mut commands: Commands,
    assets: Res<AssetServer>,
    images: Res<Assets<Image>>,
    sim: Res<Sim>,
    area: Res<VisibleArea>,
    crowd: Res<VisibleCrowd>,
    mut held: ResMut<HeldSprites>,
    mut parts: Query<(&mut Transform, Option<&Sprite>), With<HeldItem>>,
) {
    let state = &sim.0;

    // Anyone who put it down, swapped it for something else, left the world,
    // or left the canvas. That last one matters: an item is drawn beside its
    // carrier rather than as a child of it, so a culled carrier would
    // otherwise leave a burger floating where it used to be.
    held.by_uid.retain(|uid, &mut (sprite, kind)| {
        let still = state.entities().get(*uid).is_some_and(|entity| {
            entity
                .inventory()
                .and_then(|inventory| inventory.hand())
                == Some(kind)
                && area.should_keep(world_pos(entity.position()))
        });
        if !still {
            commands.entity(sprite).despawn();
        }
        still
    });

    // Anyone visible holding something, new or continuing.
    for &slot in crowd.slots() {
        let Some(entity) = state.entities().slot(slot) else {
            continue;
        };
        let Some(kind) = entity.inventory().and_then(|inventory| inventory.hand()) else {
            continue;
        };
        let uid = entity.uid();
        let pos = world_pos(entity.position());
        // The depth is the carrier's own, not the item's: it is the person who
        // is in front of or behind somebody else, and what they hold goes with
        // them.
        let at = Vec3::new(
            pos.x + HELD_X,
            pos.y + HELD_Y,
            depth_for(pos.y) + HELD_DEPTH,
        );

        match held.by_uid.get(&uid) {
            Some(&(sprite, _)) => {
                if let Ok((mut transform, texture)) = parts.get_mut(sprite) {
                    // A texture can be any size, and its size is only known
                    // once it has loaded — until then it is not drawn, so
                    // being a half pixel out for those frames shows nothing.
                    // Text has no source size and is placed as it is.
                    let nudge = texture
                        .and_then(|sprite| images.get(&sprite.image))
                        .map(|image| item::centring_nudge(image.size()))
                        .unwrap_or_default();
                    let at = at + nudge.extend(0.0);

                    // Compared before writing, as `actors::sync_sprites` does:
                    // a mutable borrow marks the transform changed, and a
                    // changed transform is propagated whether or not it moved.
                    if transform.translation != at {
                        transform.translation = at;
                    }
                }
            }
            None => {
                let sprite = spawn_held(&mut commands, &assets, kind, at);
                held.by_uid.insert(uid, (sprite, kind));
            }
        }
    }
}

/// One item, drawn however [`item::art`] says.
fn spawn_held(commands: &mut Commands, assets: &AssetServer, kind: ItemKind, at: Vec3) -> Entity {
    let mut sprite = commands.spawn((
        Name::new("held item"),
        HeldItem(kind),
        WORLD_LAYER,
        DespawnOnExit(AppState::Game),
    ));
    match item::art(kind) {
        ItemArt::Texture(path) => sprite.insert((
            Sprite::from_image(assets.load(path)),
            Transform::from_translation(at).with_scale(upscale(ART_SCALE)),
        )),
        ItemArt::Emoji(glyph) => sprite.insert((
            Text2d::new(glyph),
            TextFont {
                font: assets.load(EMOJI_FONT).into(),
                font_size: HELD_EMOJI_SIZE.into(),
                ..default()
            },
            Transform::from_translation(at),
        )),
    };
    sprite.id()
}
