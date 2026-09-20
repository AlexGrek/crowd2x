//! The props that answer to the simulation: a computer's screen comes on
//! while somebody is sitting at it, and goes dark when they leave.
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
//! That is a pass over the entities per frame, which is what [`sync_sprites`]
//! and the action bars already cost — and it is skipped entirely on a map
//! with no prop that could light up, which is every map so far but the ones
//! with a computer on them.
//!
//! [`Action::Interact`]: crate::sim::brain::Action
//! [`sync_sprites`]: super::actors

use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use crate::animation::StripAnimation;
use crate::editor::props::{Prop, Usable};
use crate::map::Point;
use crate::state::AppState;

use super::actors::Sim;

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            light_up_props_in_use.run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
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
    mut busy: Local<HashSet<Point>>,
    mut props: Query<(&Prop, &mut Usable, &mut Sprite, Option<&mut StripAnimation>)>,
) {
    if props.is_empty() {
        // No prop on this map cares, so the crowd need not be asked.
        return;
    }

    busy.clear();
    for entity in sim.0.entities().iter() {
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
