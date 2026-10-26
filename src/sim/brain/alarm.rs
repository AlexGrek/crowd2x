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
//! * **A deathdog** marks the nearest person or dog it sees outside the site — the
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
                .filter(|seen| seen.uid.kind().is_some_and(EntityType::is_deathdog_prey) && !site.contains(seen.cell));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Point, Size, FLOOR};
    use crate::sim::brain::perception::Perception;
    use crate::sim::testing::World;
    use crate::sim::uid::Uid;

    #[test]
    fn a_deathdog_marks_a_dog_it_sees_outside_as_prey() {
        let world_map = Map::new(Size::new(12, 12), FLOOR).with_wasteland(6);
        let mut world = World::new(world_map);
        let prey = Uid::new(EntityType::Dog, 51);
        world.occupancy.claim(Point::new(-8, 3), prey).unwrap();
        let body = Body::at_cell(Uid::new(EntityType::Deathdog, 52), Point::new(-8, 6));
        let seen = Perception::default().look(&world.ctx(), &body);
        let mut memory = Memory::default();
        take_in(&world.ctx(), &body, &seen, &mut memory);
        assert_eq!(memory.spotted().map(|s| s.who), Some(prey));
    }
}
