//! What in a look is danger, or prey — brain step 0, after
//! [`Perception::look`](super::perception::Perception::look).
//!
//! **Only outside the site.** Inside it nobody is in danger
//! (`sim::deathdogs`), so somebody inside reacts to nothing here — not to a
//! deathdog over the wall, not to somebody running in at the gate.
//!
//! * **A person outside** is scared by the nearest deathdog it sees, or the
//!   nearest person it sees running in a panic ([`Think::panics`]) — which is
//!   how **panic spreads by sight**, one onlooker to the next — and remembers
//!   who scared it, where and when ([`Memory::spot`]). `FleeRoutine` does the
//!   rest.
//! * **A deathdog** marks the nearest person it sees outside the site — the
//!   one it is already after, if it sees that one again — and `HuntRoutine`
//!   does the rest.
//!
//! The sightings are the look's own, nearest first, so this is a walk of a
//! few on the stack: no scan of the crowd, nothing allocated.

use crate::sim::entity::{Body, Think};
use crate::sim::uid::EntityType;

use super::memory::{Memory, Spotted};
use super::perception::{Sighting, Sightings};

/// React to one look's `sightings` — see the module docs.
pub fn take_in(ctx: &Think<'_>, body: &Body, sightings: &Sightings, memory: &mut Memory) {
    let site = ctx.map.site();
    let spot = |memory: &mut Memory, seen: Sighting| {
        memory.spot(Spotted { who: seen.uid, at: seen.cell, when: ctx.clock.elapsed() });
    };
    match body.uid().kind() {
        Some(EntityType::Human) => {
            if site.contains(body.center_position()) {
                return;
            }
            let scary = sightings.iter().find(|seen| match seen.uid.kind() {
                Some(EntityType::Deathdog) => true,
                Some(EntityType::Human) => ctx.panics.contains(seen.uid),
                _ => false,
            });
            if let Some(seen) = scary {
                spot(memory, seen);
            }
        }
        Some(EntityType::Deathdog) => {
            let quarry = memory.spotted().map(|spotted| spotted.who);
            let mut prey = sightings
                .iter()
                .filter(|seen| seen.uid.kind() == Some(EntityType::Human) && !site.contains(seen.cell));
            let first = prey.next();
            let chosen = match first {
                Some(seen) if Some(seen.uid) == quarry => Some(seen),
                Some(seen) => Some(prey.find(|seen| Some(seen.uid) == quarry).unwrap_or(seen)),
                None => None,
            };
            if let Some(seen) = chosen {
                spot(memory, seen);
            }
        }
        _ => {}
    }
}
