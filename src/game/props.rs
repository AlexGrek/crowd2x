//! The props that answer to the simulation: a computer's screen comes on
//! while somebody is sitting at it, and goes dark when they leave; a door's
//! leaf slides open and shut as the simulation moves it ([`swing_doors`]).
//!
//! The art is not this module's — a prop's pictures are both on its palette
//! entry ([`PaletteItem::in_use`](crate::editor::PaletteItem::in_use)), drawn
//! by `editor::props` for whichever screen asked for them. What is *here* is
//! the one thing the editor cannot know, because nothing is simulated there:
//! which of them is showing.
//!
//! # In use is a fact about the unit, not about the prop
//!
//! A feature has no state of its own — that is what lets the feature index be
//! built once with the world and read from every thread — so "is this
//! computer busy" is not a question the simulation can be asked. It is asked
//! of the crowd instead: whoever is mid-[`Action::Interact`] names the cell
//! they are using ([`GameEntity::interacting_with`]), and a prop standing in
//! one of those cells is in use.
//!
//! Only the units on the canvas are asked ([`VisibleCrowd`]), not the whole
//! crowd, because only the props on the canvas are drawn: `editor::props`
//! culls them to the view, so a prop with a sprite is one on screen, and
//! whoever is using it is beside it or standing in it. It is skipped entirely
//! on a view with no prop that could light up.
//!
//! One seam is left, at the very edge of the canvas. A unit not already drawn
//! is collected only once its position is within half a cell of the canvas,
//! so a computer showing at most half of itself at the edge, used from the
//! cell beyond it, stays dark until the camera has moved another half cell.
//! Closing it means collecting a wider ring of units than the ones drawn,
//! which is a second list on [`VisibleCrowd`] for the sake of one consumer.
//!
//! [`Action::Interact`]: crate::sim::brain::Action

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::animation::StripAnimation;
use crate::editor::props::{DoorLeaf, Prop, Usable};
use crate::map::Point;
use crate::state::AppState;

use super::actors::{Sim, VisibleCrowd};

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        // After the prop window as well as the visible crowd: a prop that has
        // just scrolled into view is spawned showing its idle strip, and is
        // put right on the frame it appears rather than the one after.
        app.add_systems(
            Update,
            (light_up_props_in_use, swing_doors)
                .in_set(super::SpriteSync)
                .after(crate::editor::props::sync_prop_window)
                .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
        );
    }
}

/// Swap each usable prop between its two strips, from whether anybody is
/// using its cell this frame.
///
/// The set of busy cells is a [`Local`] rather than a resource or a fresh
/// allocation: it is cleared and refilled every frame, and clearing a
/// `HashSet` keeps the memory it has already claimed, so after the first few
/// frames this allocates nothing.
fn light_up_props_in_use(
    sim: Res<Sim>,
    crowd: Res<VisibleCrowd>,
    mut busy: Local<HashSet<Point>>,
    mut props: Query<(&Prop, &mut Usable, &mut Sprite, Option<&mut StripAnimation>)>,
) {
    if props.is_empty() {
        // No prop on screen cares, so the crowd need not be asked.
        return;
    }

    busy.clear();
    for &slot in crowd.slots() {
        let Some(entity) = sim.0.entities().slot(slot) else {
            continue;
        };
        if let Some(cell) = entity.interacting_with() {
            busy.insert(cell);
        }
    }

    for (prop, mut usable, mut sprite, anim) in &mut props {
        let Some((image, frames)) = usable.show(busy.contains(&prop.cell())) else {
            // Already showing the right one, which is what it is almost
            // every frame — a screen changes state twice a visit.
            continue;
        };
        sprite.image = image;
        if let Some(mut anim) = anim {
            // The two strips need not be the same length, and the new one
            // starts at its first frame rather than wherever the old one had
            // got to.
            anim.restart(frames);
            sprite.rect = Some(anim.rect());
        }
    }
}

/// Show every door on screen as open as the simulation says it is: one frame
/// of its strip, shut first and gone last ([`DoorLeaf`]).
///
/// A fact about the door itself (`sim::door::Doors`), not about whoever is
/// going through it, so unlike a computer's screen the crowd is not asked.
/// Each leaf on the canvas is a binary search over the doors, and a write
/// only on a frame it moved.
fn swing_doors(sim: Res<Sim>, mut doors: Query<(&Prop, &mut DoorLeaf, &mut Sprite)>) {
    let state = sim.0.doors();
    for (prop, mut leaf, mut sprite) in &mut doors {
        let last = leaf.frames.saturating_sub(1);
        let frame = ((state.openness(prop.cell()) * last as f32).round() as u32).min(last);
        if frame == leaf.showing {
            continue;
        }
        leaf.showing = frame;
        if let Some(rect) = sprite.rect.as_mut() {
            let side = rect.width();
            *rect = Rect::new(frame as f32 * side, 0.0, (frame + 1) as f32 * side, side);
        }
    }
}
