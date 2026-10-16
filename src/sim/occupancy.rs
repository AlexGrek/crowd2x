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
//!
//! # Off the site
//!
//! The world round an endless map's site goes on for ever
//! ([`crate::map::wasteland`]), and a unit can walk out into it, so this layer
//! covers every cell there is: the site densely, as it always has, and the
//! wasteland a [`crate::map::chunk`] at a time, a chunk coming into being the
//! first time somebody stands in it. The spawn pass makes the chunks round
//! everybody out there ahead of them ([`Occupancy::prepare`]), so the move
//! step — which must not allocate — finds the chunk it claims in already
//! there; a unit that outran that (a long run of ticks with no spawn pass
//! between, which only a test does) costs one allocation, never a wrong
//! answer.

use crate::map::chunk::{local_index, ChunkGrid, ChunkPos, CHUNK_AREA};
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
    /// The same, one bit per cell: whether anybody is there at all, kept in
    /// step by every write to `cells`. What a unit looking round scans
    /// (`brain::perception`): seventy cells of it are a few words in the
    /// nearest cache, where seventy cells of `cells` are some fifty lines of
    /// a crowd-sized array — and most of them are empty, which is all the
    /// look needs to know about them.
    occupied: Vec<u64>,
    /// Everybody off the site, a chunk at a time.
    wild: ChunkGrid<WildCells>,
}

/// One chunk of [`Occupancy`] off the site: the same two arrays as the site's,
/// for one chunk's cells.
#[derive(Clone)]
struct WildCells {
    cells: [Option<Uid>; CHUNK_AREA],
    occupied: [u64; CHUNK_AREA / 64],
}

impl WildCells {
    fn empty() -> Box<WildCells> {
        Box::new(WildCells { cells: [None; CHUNK_AREA], occupied: [0; CHUNK_AREA / 64] })
    }

    fn set(&mut self, index: usize, uid: Option<Uid>) {
        self.cells[index] = uid;
        if uid.is_some() {
            self.occupied[index / 64] |= 1 << (index % 64);
        } else {
            self.occupied[index / 64] &= !(1 << (index % 64));
        }
    }
}

impl Occupancy {
    /// An empty layer covering a map of `size`.
    pub fn new(size: Size) -> Occupancy {
        Occupancy {
            size,
            cells: vec![None; size.area()],
            occupied: vec![0; size.area().div_ceil(64)],
            wild: ChunkGrid::new(),
        }
    }

    /// Make the chunks within one chunk of `cell`, if it is off the site, so
    /// a unit standing there can step anywhere near without the move step
    /// having to allocate. The spawn pass's job; cheap when they are there.
    pub fn prepare(&mut self, cell: Point) {
        if self.size.contains(cell) {
            return;
        }
        for pos in ChunkPos::of(cell).around(1) {
            if !self.wild.contains(pos) {
                self.wild.get_or_insert_with(pos, WildCells::empty);
            }
        }
    }

    /// How many chunks of the wasteland this layer holds.
    pub fn wild_chunks(&self) -> usize {
        self.wild.len()
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Who is standing in `cell`, if anybody.
    ///
    /// Every cell in the world can be asked: off the site, a chunk nobody
    /// has stood in holds nobody.
    #[inline]
    pub fn occupant(&self, cell: Point) -> Option<Uid> {
        match self.size.index_of(cell) {
            Some(index) => self.cells[index],
            None => self.wild.at(cell)?.cells[local_index(cell)],
        }
    }

    /// Who is standing at a row-major index the caller has already worked
    /// out — the scan-friendly form of [`Occupancy::occupant`]. An index past
    /// the end panics.
    #[inline]
    pub fn occupant_at(&self, index: usize) -> Option<Uid> {
        self.cells[index]
    }

    /// Whether anybody stands at a row-major index — one bit, for a scan to
    /// pass over an empty cell without reading [`Occupancy::occupant_at`].
    #[inline]
    pub fn is_occupied_at(&self, index: usize) -> bool {
        self.occupied[index / 64] & (1 << (index % 64)) != 0
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
    /// Off the site the claim is recorded in the cell's chunk, made now if
    /// nobody has stood in it before.
    pub fn claim(&mut self, cell: Point, uid: Uid) -> Result<(), Uid> {
        let Some(index) = self.size.index_of(cell) else {
            if let Some(other) = self.occupant(cell).filter(|&other| other != uid) {
                return Err(other);
            }
            self.wild
                .get_or_insert_with(ChunkPos::of(cell), WildCells::empty)
                .set(local_index(cell), Some(uid));
            return Ok(());
        };
        match self.cells[index] {
            Some(other) if other != uid => Err(other),
            _ => {
                self.cells[index] = Some(uid);
                self.occupied[index / 64] |= 1 << (index % 64);
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
            let Some(chunk) = self.wild.get_mut(ChunkPos::of(cell)) else {
                return false;
            };
            let index = local_index(cell);
            if chunk.cells[index] != Some(uid) {
                return false;
            }
            chunk.set(index, None);
            return true;
        };
        if self.cells[index] == Some(uid) {
            self.cells[index] = None;
            self.occupied[index / 64] &= !(1 << (index % 64));
            true
        } else {
            false
        }
    }

    /// How many cells have somebody in them. Not the number of entities —
    /// entities sharing a cell after a spawn are counted once.
    pub fn count_occupied(&self) -> usize {
        let wild: usize = self
            .wild
            .iter()
            .map(|(_, chunk)| chunk.occupied.iter().map(|word| word.count_ones() as usize).sum::<usize>())
            .sum();
        self.cells.iter().filter(|cell| cell.is_some()).count() + wild
    }

    pub fn clear(&mut self) {
        self.cells.fill(None);
        self.occupied.fill(0);
        for chunk in self.wild.iter_mut() {
            chunk.cells.fill(None);
            chunk.occupied.fill(0);
        }
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
    fn cells_off_the_site_are_held_like_any_other() {
        // The wasteland round an endless map is walked on, so somebody out
        // there is in somebody else's way exactly as on the site.
        let mut occupancy = layer();
        let outside = [Point::new(-1, 0), Point::new(4, 0), Point::new(0, 3), Point::new(-5000, 12000)];
        for (i, &cell) in outside.iter().enumerate() {
            assert_eq!(occupancy.occupant(cell), None);
            assert_eq!(occupancy.claim(cell, uid(i as u64 + 1)), Ok(()));
            assert_eq!(occupancy.occupant(cell), Some(uid(i as u64 + 1)));
            assert_eq!(occupancy.claim(cell, uid(99)), Err(uid(i as u64 + 1)));
        }
        assert_eq!(occupancy.count_occupied(), outside.len());
        assert!(!occupancy.release(outside[0], uid(99)), "not theirs to give up");
        for (i, &cell) in outside.iter().enumerate() {
            assert!(occupancy.release(cell, uid(i as u64 + 1)));
            assert_eq!(occupancy.occupant(cell), None);
        }
        assert_eq!(occupancy.count_occupied(), 0);
        assert!(!occupancy.release(Point::new(900, 900), uid(1)), "a chunk nobody stood in");
    }

    #[test]
    fn preparing_makes_the_chunks_round_somebody_and_nothing_on_the_site() {
        let mut occupancy = layer();
        occupancy.prepare(Point::new(1, 1));
        assert_eq!(occupancy.wild_chunks(), 0, "the site needs no chunks");
        occupancy.prepare(Point::new(-40, 10));
        assert_eq!(occupancy.wild_chunks(), 9);
        occupancy.prepare(Point::new(-41, 11));
        assert_eq!(occupancy.wild_chunks(), 9, "the same chunks");
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

    /// The bitmap a look scans says exactly what `cells` says, through every
    /// kind of write.
    #[test]
    fn the_occupied_bits_follow_every_claim_release_and_clear() {
        let mut occupancy = Occupancy::new(Size::new(70, 3));
        let agrees = |occupancy: &Occupancy| {
            occupancy
                .size()
                .points()
                .all(|cell| occupancy.is_occupied_at(occupancy.size().index_of(cell).unwrap()) == occupancy.is_occupied(cell))
        };
        for (i, x) in [0, 63, 64, 69].into_iter().enumerate() {
            occupancy.claim(Point::new(x, 1), uid(i as u64 + 1)).expect("empty");
        }
        assert!(agrees(&occupancy));
        assert!(occupancy.is_occupied_at(occupancy.size().index_of(Point::new(64, 1)).unwrap()));
        assert_eq!(occupancy.claim(Point::new(63, 1), uid(9)), Err(uid(2)));
        assert!(!occupancy.release(Point::new(64, 1), uid(9)), "not theirs to give up");
        assert!(agrees(&occupancy));
        assert!(occupancy.release(Point::new(64, 1), uid(3)));
        assert!(agrees(&occupancy));
        occupancy.clear();
        assert!(agrees(&occupancy));
        assert_eq!(occupancy.count_occupied(), 0);
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
