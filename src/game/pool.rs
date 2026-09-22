//! Sprite bodies parked for reuse.
//!
//! A unit walking across the edge of the view would otherwise cost a spawn and
//! a despawn of four entities every time it crossed, and a camera panning over
//! a crowd crosses a great many at once. Parking a body instead — hiding it and
//! taking [`Actor`] off it — turns that into a `Vec` push and a handful of
//! component writes, and `characters::human::redress` is what dresses it as
//! whoever needs it next.
//!
//! # Why it has to be bounded
//!
//! Bevy's `extract_sprites` walks every sprite entity in the main world each
//! frame and only *then* skips the invisible ones. **A parked body is cheaper
//! than a spawn and it is not free**, so a pool that kept everything it was
//! ever handed would grow to the size of the crowd and give back the whole of
//! what culling won. [`park_budget`] is the cap, taken from the size of the
//! view rather than from a constant, because what the view can hold is exactly
//! what the pool can usefully hold.
//!
//! [`Actor`]: super::actors::Actor

use bevy::prelude::*;

/// The most bodies of one kind kept parked.
///
/// A quarter of the cells on screen: the visible crowd is at most one unit per
/// cell (`sim::Occupancy` allows no more), and in practice far fewer, so a
/// quarter absorbs a camera sweeping across a busy room without keeping a body
/// for a cell that has never had one. The floor stops a deeply zoomed-in view
/// from throwing away bodies it is about to want again; the ceiling stops a
/// zoomed-right-out one from holding hundreds.
pub fn park_budget(view_cells: usize) -> usize {
    (view_cells / 4).clamp(16, 256)
}

/// Bodies waiting to be dressed as somebody.
///
/// Two lists, because a human body is a root plus three layer children and a
/// dog body is one entity: neither can become the other, so they cannot share
/// a pool.
#[derive(Resource, Default)]
pub struct ActorPool {
    humans: Vec<Entity>,
    dogs: Vec<Entity>,
}

impl ActorPool {
    pub fn take_human(&mut self) -> Option<Entity> {
        self.humans.pop()
    }

    pub fn take_dog(&mut self) -> Option<Entity> {
        self.dogs.pop()
    }

    /// Park a human body, or say it should be despawned instead.
    ///
    /// Returns `false` when the pool is full, and the caller despawns — the
    /// bound is the point of the whole structure, so it is enforced here
    /// rather than trusted to a caller.
    #[must_use]
    pub fn park_human(&mut self, body: Entity, budget: usize) -> bool {
        park(&mut self.humans, body, budget)
    }

    #[must_use]
    pub fn park_dog(&mut self, body: Entity, budget: usize) -> bool {
        park(&mut self.dogs, body, budget)
    }

    /// Forget every parked body **without despawning it**.
    ///
    /// Called on the way out of the game screen, where `DespawnOnExit` is
    /// already taking the entities themselves: despawning here as well would
    /// be a second command against an entity Bevy is in the middle of
    /// removing.
    pub fn clear(&mut self) {
        self.humans.clear();
        self.dogs.clear();
    }

    /// How many bodies are parked, for the assertion that the pool stayed
    /// bounded.
    pub fn len(&self) -> usize {
        self.humans.len() + self.dogs.len()
    }
}

fn park(pool: &mut Vec<Entity>, body: Entity, budget: usize) -> bool {
    if pool.len() >= budget {
        return false;
    }
    pool.push(body);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_is_clamped_at_both_ends() {
        // Zoomed right in: a handful of cells, but still worth keeping a few.
        assert_eq!(park_budget(0), 16);
        assert_eq!(park_budget(8), 16);
        // The default zoom, about eighty cells.
        assert_eq!(park_budget(80), 20);
        // Zoomed right out on a huge map, and still bounded.
        assert_eq!(park_budget(540), 135);
        assert_eq!(park_budget(100_000), 256);
    }

    #[test]
    fn the_budget_never_shrinks_as_the_view_grows() {
        let mut last = 0;
        for cells in (0..2000).step_by(7) {
            let budget = park_budget(cells);
            assert!(budget >= last, "{cells} cells went backwards");
            last = budget;
        }
    }

    /// The bound is enforced by the pool, not by the caller remembering to
    /// check it — that is what stops it growing to crowd size.
    #[test]
    fn a_full_pool_refuses_instead_of_growing() {
        let mut pool = ActorPool::default();
        let budget = 3;
        for i in 0..budget {
            assert!(
                pool.park_human(Entity::from_raw_u32(i as u32).unwrap(), budget),
                "refused with room left"
            );
        }
        assert!(!pool.park_human(Entity::from_raw_u32(99).unwrap(), budget));
        assert_eq!(pool.len(), budget);

        // ...and the two kinds are counted apart, since neither can be dressed
        // as the other.
        assert!(pool.park_dog(Entity::from_raw_u32(100).unwrap(), budget));
        assert_eq!(pool.len(), budget + 1);
    }

    #[test]
    fn taking_gives_back_what_was_parked_and_then_nothing() {
        let mut pool = ActorPool::default();
        let body = Entity::from_raw_u32(1).unwrap();
        assert!(pool.park_human(body, 16));
        assert_eq!(pool.take_human(), Some(body));
        assert_eq!(pool.take_human(), None);
        // A human body is never handed out as a dog.
        assert!(pool.park_human(body, 16));
        assert_eq!(pool.take_dog(), None);
    }
}
