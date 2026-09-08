//! Who is standing where: the **dynamic** half of passability.
//!
//! [`crate::map::PassabilityMap`] answers what the terrain allows, and says in
//! its own docs why it stops there — "*static* as in not the agents ...
//! anything dynamic — who is standing where, a door someone is holding shut —
//! belongs in a separate layer that a query consults after this one". This is
//! that layer, and a move consults the two in that order: the terrain first,
//! because a wall is cheaper to hit than a crowd.
//!
//! # One entity to a cell
//!
//! Occupancy is tracked **at the tile level**, keyed on the cell an entity's
//! centre is in ([`crate::sim::entity::Body::center_position`]) and nothing
//! finer. An entity is not a circle with a radius here; it is standing in cell
//! `(3, 2)` or it is not. Two entities inside one cell at different sub-cell
//! positions would still be in the same cell, so that is not a distinction
//! this layer can make, and pretending otherwise is how a tile-based crowd
//! grows a physics engine it did not ask for.
//!
//! # Movement never creates an overlap; spawning can
//!
//! A move into an occupied cell is refused and names the occupant — that
//! refusal is the whole point of the layer, and it is enforced in one place
//! (the move step of [`crate::sim::process_pass`]).
//!
//! Spawning is the exception, and deliberately: [`crate::sim::GameState::spawn`]
//! puts an entity exactly where it was asked for, passable or not and occupied
//! or not, because a caller that wanted a legal spawn point should have picked
//! one and silently moving an entity elsewhere makes a test that spawns at a
//! known cell lie. So a cell can hold a second, *unregistered* entity — this
//! layer names the first one — and [`Occupancy::release`] only ever clears a
//! cell whose occupant is the entity leaving it, so the newcomer's arrival
//! cannot evict anybody and the original's departure cannot hand the cell to
//! it either. A cell that never held an unregistered entity can therefore
//! never come to hold two.

use crate::map::{Point, Size};

use super::uid::Uid;

/// The cell each entity is standing in, one entity to a cell.
///
/// Dense rather than a `HashMap<Point, Uid>`: this is read on every attempted
/// step and written on every completed one, and a row of cells in one cache
/// line is the same reason the passability map is a bitset. `Option<Uid>` is
/// eight bytes and not nine, because [`Uid`] is a `NonZeroU64` — an empty cell
/// is a zero word.
#[derive(Clone)]
pub struct Occupancy {
    size: Size,
    cells: Vec<Option<Uid>>,
}

impl Occupancy {
    /// An empty layer covering a map of `size`.
    pub fn new(size: Size) -> Occupancy {
        Occupancy {
            size,
            cells: vec![None; size.area()],
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Who is standing in `cell`, if anybody.
    ///
    /// Off the map is nobody, so callers need no bounds check of their own —
    /// the same rule [`crate::map::PassabilityMap::is_passable`] follows.
    pub fn occupant(&self, cell: Point) -> Option<Uid> {
        self.cells[self.size.index_of(cell)?]
    }

    pub fn is_occupied(&self, cell: Point) -> bool {
        self.occupant(cell).is_some()
    }

    /// Whether `uid` could stand in `cell` as far as *this* layer is concerned.
    ///
    /// True for a cell it already occupies: an entity is not in its own way,
    /// and a step that stays inside one cell must not be refused by the
    /// entity taking it.
    pub fn is_free_for(&self, cell: Point, uid: Uid) -> bool {
        match self.occupant(cell) {
            Some(occupant) => occupant == uid,
            None => true,
        }
    }

    /// Take `cell` for `uid`.
    ///
    /// `Err(other)` — **the obstacle's id** — when somebody else is already
    /// there, and nothing is written: a refused claim leaves the layer exactly
    /// as it found it, so the caller can cancel the move without unwinding
    /// anything.
    ///
    /// Off the map there is nothing to claim and nothing is recorded. That is
    /// not reachable from the tick, which asks the terrain first and every
    /// cell outside the map is impassable.
    pub fn claim(&mut self, cell: Point, uid: Uid) -> Result<(), Uid> {
        let Some(index) = self.size.index_of(cell) else {
            return Ok(());
        };
        match self.cells[index] {
            Some(other) if other != uid => Err(other),
            _ => {
                self.cells[index] = Some(uid);
                Ok(())
            }
        }
    }

    /// Give `cell` up, if `uid` is the one holding it. Returns whether it was.
    ///
    /// Conditional rather than a blind clear, and that is what keeps a spawned
    /// overlap contained: an entity standing in a cell it never registered
    /// must not be able to evict the entity that did.
    pub fn release(&mut self, cell: Point, uid: Uid) -> bool {
        let Some(index) = self.size.index_of(cell) else {
            return false;
        };
        if self.cells[index] == Some(uid) {
            self.cells[index] = None;
            true
        } else {
            false
        }
    }

    /// How many cells have somebody in them. Not the number of entities —
    /// entities sharing a cell after a spawn are counted once.
    pub fn count_occupied(&self) -> usize {
        self.cells.iter().filter(|cell| cell.is_some()).count()
    }

    pub fn clear(&mut self) {
        self.cells.fill(None);
    }
}

impl std::fmt::Debug for Occupancy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Occupancy")
            .field("size", &self.size)
            .field("occupied", &self.count_occupied())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::uid::EntityType;

    fn uid(body: u64) -> Uid {
        Uid::new(EntityType::Human, body)
    }

    fn layer() -> Occupancy {
        Occupancy::new(Size::new(4, 3))
    }

    #[test]
    fn a_fresh_layer_holds_nobody() {
        let occupancy = layer();
        assert_eq!(occupancy.count_occupied(), 0);
        assert!(occupancy.size().points().all(|cell| !occupancy.is_occupied(cell)));
    }

    #[test]
    fn a_claimed_cell_names_who_took_it() {
        let mut occupancy = layer();
        let cell = Point::new(2, 1);

        assert_eq!(occupancy.claim(cell, uid(1)), Ok(()));
        assert_eq!(occupancy.occupant(cell), Some(uid(1)));
        assert_eq!(occupancy.count_occupied(), 1);
        // ...and only that cell.
        assert!(!occupancy.is_occupied(Point::new(1, 1)));
    }

    #[test]
    fn claiming_an_occupied_cell_fails_with_the_occupants_id_and_writes_nothing() {
        // The whole layer in one test: a collision names the obstacle, and the
        // refusal is total, so the caller can cancel a move without repair.
        let mut occupancy = layer();
        let cell = Point::new(0, 0);
        occupancy.claim(cell, uid(1)).expect("empty");

        assert_eq!(occupancy.claim(cell, uid(2)), Err(uid(1)));
        assert_eq!(occupancy.occupant(cell), Some(uid(1)));
        assert!(!occupancy.is_free_for(cell, uid(2)));
    }

    #[test]
    fn an_entity_is_never_in_its_own_way() {
        let mut occupancy = layer();
        let cell = Point::new(3, 2);
        occupancy.claim(cell, uid(1)).expect("empty");

        assert!(occupancy.is_free_for(cell, uid(1)));
        assert_eq!(occupancy.claim(cell, uid(1)), Ok(()));
        assert_eq!(occupancy.count_occupied(), 1);
    }

    #[test]
    fn only_the_entity_holding_a_cell_can_give_it_up() {
        // What contains a spawned overlap: the unregistered entity standing in
        // somebody else's cell must not be able to free it.
        let mut occupancy = layer();
        let cell = Point::new(1, 2);
        occupancy.claim(cell, uid(1)).expect("empty");

        assert!(!occupancy.release(cell, uid(2)));
        assert_eq!(occupancy.occupant(cell), Some(uid(1)));

        assert!(occupancy.release(cell, uid(1)));
        assert_eq!(occupancy.occupant(cell), None);
        assert!(!occupancy.release(cell, uid(1)), "releasing twice");
    }

    #[test]
    fn cells_off_the_map_hold_nobody_and_swallow_a_claim() {
        let mut occupancy = layer();
        for outside in [Point::new(-1, 0), Point::new(4, 0), Point::new(0, 3)] {
            assert_eq!(occupancy.claim(outside, uid(1)), Ok(()));
            assert_eq!(occupancy.occupant(outside), None);
            assert!(!occupancy.release(outside, uid(1)));
        }
        // ...and asking did not quietly widen the layer.
        assert_eq!(occupancy.count_occupied(), 0);
    }

    #[test]
    fn walking_a_cell_across_the_layer_leaves_one_occupant_behind() {
        let mut occupancy = layer();
        let mut here = Point::new(0, 0);
        occupancy.claim(here, uid(1)).expect("empty");

        for x in 1..4 {
            let next = Point::new(x, 0);
            occupancy.claim(next, uid(1)).expect("empty");
            occupancy.release(here, uid(1));
            here = next;
            assert_eq!(occupancy.count_occupied(), 1);
            assert_eq!(occupancy.occupant(here), Some(uid(1)));
        }
    }

    #[test]
    fn clearing_empties_every_cell() {
        let mut occupancy = layer();
        occupancy.claim(Point::new(1, 1), uid(1)).expect("empty");
        occupancy.claim(Point::new(2, 2), uid(2)).expect("empty");
        occupancy.clear();
        assert_eq!(occupancy.count_occupied(), 0);
    }
}
