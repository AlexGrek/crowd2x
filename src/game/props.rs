//! The props that answer to the simulation: a computer's screen comes on
//! while somebody is sitting at it, and goes dark when they leave; a door's
//! leaf slides open and shut as the simulation moves it ([`swing_doors`]);
//! a light switch's rocker shows whether its room's lights are on
//! ([`flip_switches`]); a scrap bin fills up as the collectors store scrap in
//! it ([`fill_bins`]).
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
use crate::editor::props::{BinFill, DoorLeaf, Prop, SwitchPlate, Usable};
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
            (light_up_props_in_use, swing_doors, flip_switches, fill_bins)
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
        show_frame(&mut sprite, frame);
    }
}

/// Show every light switch on screen on or off, as the simulation says its
/// room is: the first frame of its strip on, the second off. A binary search
/// per plate on the canvas, and a write only on a frame one was flipped.
fn flip_switches(sim: Res<Sim>, mut plates: Query<(&Prop, &mut SwitchPlate, &mut Sprite)>) {
    for (prop, mut plate, mut sprite) in &mut plates {
        let on = sim.0.light_is_on(prop.cell());
        if on == plate.showing_on {
            continue;
        }
        plate.showing_on = on;
        show_frame(&mut sprite, u32::from(!on));
    }
}

/// The frame of a bin's strip for `stored` pieces: empty, a few (up to
/// five, a packful or so), half full (up to twenty), heaped.
pub fn bin_frame(stored: u32) -> u32 {
    match stored {
        0 => 0,
        1..=5 => 1,
        6..=20 => 2,
        _ => 3,
    }
}

/// Show every store on screen — a scrap bin, a steel rack, a scrap hopper, a
/// steel pallet — as full as the simulation says it is
/// ([`bin_frame`]). A binary search per bin on the canvas, and a write only
/// on a frame somebody filled it past a step.
fn fill_bins(sim: Res<Sim>, mut bins: Query<(&Prop, &mut BinFill, &mut Sprite)>) {
    let state = sim.0.bins();
    for (prop, mut fill, mut sprite) in &mut bins {
        let frame = bin_frame(state.stored(prop.cell()));
        if frame == fill.showing {
            continue;
        }
        fill.showing = frame;
        show_frame(&mut sprite, frame);
    }
}

/// Put frame `frame` of a posed strip on its sprite.
fn show_frame(sprite: &mut Sprite, frame: u32) {
    if let Some(rect) = sprite.rect.as_mut() {
        let side = rect.width();
        *rect = Rect::new(frame as f32 * side, 0.0, (frame + 1) as f32 * side, side);
    }
}

#[cfg(test)]
mod tests {
    use super::bin_frame;

    #[test]
    fn a_bin_shows_fuller_the_more_is_stored_in_it() {
        let frames: Vec<u32> = [0, 1, 3, 5, 6, 20, 21, 500].into_iter().map(bin_frame).collect();
        assert_eq!(frames, [0, 1, 1, 1, 2, 2, 3, 3]);
    }
}
