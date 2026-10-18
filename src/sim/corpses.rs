//! Where the dead lie: [`Corpses`], what an undertaker looks for.
//!
//! A unit that dies ([`Effect::Died`](super::Effect::Died)) is filed here by
//! the world step, at the cell it fell in, and taken off again when an
//! undertaker takes it up ([`Effect::TakeBody`](super::Effect::TakeBody)),
//! when it leaves the world. So "the nearest body" is a walk of a short list
//! — the dead, not the crowd — the way "the nearest fridge" is a walk of the
//! fridges ([`super::feature::Features`]).
//!
//! **Not saved**: it is the dead entities and where they lie, and a world
//! restored from a save files them again ([`Corpses::of`]). Sorted by id, so
//! the order is the same however they got here.

use crate::map::Point;

use super::entities::Entities;
use super::uid::Uid;

/// Every body lying in the world, by id, and the cell it lies in.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Corpses {
    bodies: Vec<(Uid, Point)>,
}

/// No bodies anywhere: what a world without a [`super::GameState`] — a test's
/// — hands its units to look through.
pub static NONE: Corpses = Corpses::new();

impl Corpses {
    pub const fn new() -> Corpses {
        Corpses { bodies: Vec::new() }
    }

    /// Every dead entity in `entities`, where it lies.
    pub fn of(entities: &Entities) -> Corpses {
        let mut corpses = Corpses::new();
        for (_, entity) in entities.iter_slots() {
            if entity.is_dead() {
                corpses.add(entity.uid(), entity.center_position());
            }
        }
        corpses
    }

    /// `uid` lies dead in `cell`. Once: filing it again moves it.
    pub fn add(&mut self, uid: Uid, cell: Point) {
        match self.bodies.binary_search_by_key(&uid, |&(uid, _)| uid) {
            Ok(at) => self.bodies[at].1 = cell,
            Err(at) => self.bodies.insert(at, (uid, cell)),
        }
    }

    /// `uid` is not lying anywhere any more. Whether it was.
    pub fn remove(&mut self, uid: Uid) -> bool {
        match self.bodies.binary_search_by_key(&uid, |&(uid, _)| uid) {
            Ok(at) => {
                self.bodies.remove(at);
                true
            }
            Err(_) => false,
        }
    }

    /// Who lies in `cell` — the lowest id, if more than one does.
    pub fn at(&self, cell: Point) -> Option<Uid> {
        self.bodies.iter().find(|&&(_, at)| at == cell).map(|&(uid, _)| uid)
    }

    /// Every body and its cell, by id.
    pub fn iter(&self) -> impl Iterator<Item = (Uid, Point)> + '_ {
        self.bodies.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::uid::EntityType;

    #[test]
    fn bodies_are_kept_by_id_found_by_cell_and_taken_away_once() {
        let (a, b) = (Uid::new(EntityType::Human, 1), Uid::new(EntityType::Human, 2));
        let mut corpses = Corpses::new();
        corpses.add(b, Point::new(3, 3));
        corpses.add(a, Point::new(3, 3));
        assert_eq!(corpses.iter().map(|(uid, _)| uid).collect::<Vec<_>>(), [a, b]);
        assert_eq!(corpses.at(Point::new(3, 3)), Some(a), "the lower id of two in a cell");
        assert_eq!(corpses.at(Point::new(4, 3)), None);
        assert!(corpses.remove(a));
        assert!(!corpses.remove(a));
        assert_eq!(corpses.at(Point::new(3, 3)), Some(b));
        assert_eq!(corpses.len(), 1);
    }
}
