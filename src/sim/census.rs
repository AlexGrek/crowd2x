//! Who is in which chunk: the crowd, filed by [`ChunkPos`].
//!
//! [`super::occupancy::Occupancy`] answers who stands in a *cell*, one entity
//! to a cell. This answers who is anywhere in a [`crate::map::CHUNK`]-cell
//! square, **everybody** — a stack of units spawned into one cell included,
//! which the occupancy layer names only the first of. It is how the
//! simulation knows where its crowd is without walking it: which chunks have
//! anybody in them ([`Census::occupied`]), who is in one
//! ([`Census::units_in`]), whether anybody is near one
//! ([`Census::anyone_within`]). The spawn pass asks those to keep the ground
//! loaded round the crowd and to let it go behind them
//! (`GameState::follow_the_crowd`).
//!
//! **Nothing here decides anything.** The census follows the crowd; it never
//! leads it, and nobody is ticked differently for being in one chunk rather
//! than another — a unit nobody is watching, a long way off, is simulated
//! exactly as one on the screen.
//!
//! # A list per chunk, threaded through the slots
//!
//! Each chunk holds the head of a doubly linked list and its length; the
//! links themselves are one per entity [`Slot`], sized by the spawn pass like
//! the intent buffer. Moving somebody between chunks is an unlink and a link,
//! with no `Vec` growing, so the move step — which must not allocate — keeps
//! the census true as units cross from one chunk to the next. It needs the
//! chunk they step into to have a list already, so the spawn pass makes the
//! lists round every occupied chunk ahead of them ([`Census::prepare`]), as
//! it does the occupancy layer's cells; a unit that outruns that (a long run
//! of ticks with no spawn pass between, which only a test does) costs one
//! allocation, never a wrong answer.
//!
//! A list runs newest arrival first. That is the same on every run with the
//! same history, like everything else in the simulation.

use crate::map::chunk::{ChunkGrid, ChunkPos};
use crate::map::Point;

use super::entities::Slot;

/// The end of a list, and the link of a slot nobody is filed in.
const NOBODY: Slot = Slot::MAX;

/// One chunk's list: its first entry and how many there are.
#[derive(Clone, Copy, Debug)]
struct Roll {
    head: Slot,
    len: u32,
}

impl Roll {
    const EMPTY: Roll = Roll { head: NOBODY, len: 0 };
}

/// Where one slot is filed, and its neighbours in that chunk's list.
#[derive(Clone, Copy, Debug)]
struct Link {
    chunk: ChunkPos,
    prev: Slot,
    next: Slot,
    filed: bool,
}

impl Link {
    const UNFILED: Link = Link { chunk: ChunkPos::new(0, 0), prev: NOBODY, next: NOBODY, filed: false };
}

/// The crowd by chunk. See the module docs.
#[derive(Clone, Default)]
pub struct Census {
    rolls: ChunkGrid<Roll>,
    /// By slot.
    links: Vec<Link>,
    /// The lists a sweep is about to drop, kept for its capacity.
    doomed: Vec<ChunkPos>,
}

impl Census {
    pub fn new() -> Census {
        Census::default()
    }

    /// File the entity in `slot`, standing in `cell`: it has just arrived.
    pub fn file(&mut self, slot: Slot, cell: Point) {
        let at = slot as usize;
        if self.links.len() <= at {
            self.links.resize(at + 1, Link::UNFILED);
        }
        if self.links[at].filed {
            self.unlink(slot);
        }
        self.link(slot, ChunkPos::of(cell));
    }

    /// Take the entity in `slot` off the census: it has gone.
    pub fn strike(&mut self, slot: Slot) {
        if self.links.get(slot as usize).is_some_and(|link| link.filed) {
            self.unlink(slot);
        }
    }

    /// The entity in `slot` now stands in `cell`: move it to that chunk's
    /// list if it has crossed into another. The move step's call, after a
    /// step into a new cell; it allocates nothing when the list it joins is
    /// there already ([`Census::prepare`]).
    #[inline]
    pub fn moved(&mut self, slot: Slot, cell: Point) {
        let pos = ChunkPos::of(cell);
        let Some(&link) = self.links.get(slot as usize) else {
            return;
        };
        if !link.filed || link.chunk == pos {
            return;
        }
        self.unlink(slot);
        self.link(slot, pos);
    }

    fn link(&mut self, slot: Slot, pos: ChunkPos) {
        let roll = self.rolls.get_or_insert_with(pos, || Roll::EMPTY);
        let head = roll.head;
        roll.head = slot;
        roll.len += 1;
        if head != NOBODY {
            self.links[head as usize].prev = slot;
        }
        self.links[slot as usize] = Link { chunk: pos, prev: NOBODY, next: head, filed: true };
    }

    fn unlink(&mut self, slot: Slot) {
        let link = self.links[slot as usize];
        let roll = self.rolls.get_mut(link.chunk).expect("a filed slot's chunk has a list");
        roll.len -= 1;
        if link.prev == NOBODY {
            roll.head = link.next;
        } else {
            self.links[link.prev as usize].next = link.next;
        }
        if link.next != NOBODY {
            self.links[link.next as usize].prev = link.prev;
        }
        self.links[slot as usize] = Link::UNFILED;
    }

    /// The chunk the entity in `slot` is filed in.
    pub fn chunk_of(&self, slot: Slot) -> Option<ChunkPos> {
        self.links.get(slot as usize).filter(|link| link.filed).map(|link| link.chunk)
    }

    /// How many are in the chunk at `pos`.
    pub fn count(&self, pos: ChunkPos) -> usize {
        self.rolls.get(pos).map_or(0, |roll| roll.len as usize)
    }

    /// The slots of everybody in the chunk at `pos`, newest arrival first.
    pub fn units_in(&self, pos: ChunkPos) -> impl Iterator<Item = Slot> + '_ {
        let mut next = self.rolls.get(pos).map_or(NOBODY, |roll| roll.head);
        std::iter::from_fn(move || {
            (next != NOBODY).then(|| {
                let slot = next;
                next = self.links[slot as usize].next;
                slot
            })
        })
    }

    /// Every chunk with anybody in it.
    pub fn occupied(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.rolls.iter().filter(|(_, roll)| roll.len > 0).map(|(pos, _)| pos)
    }

    /// Whether anybody is within `radius` chunks of `pos`, that chunk
    /// included.
    pub fn anyone_within(&self, pos: ChunkPos, radius: i32) -> bool {
        pos.around(radius).any(|near| self.count(near) > 0)
    }

    /// How many chunks have a list, empty ones included.
    pub fn lists(&self) -> usize {
        self.rolls.len()
    }

    /// Make the lists of the chunk at `around` and the eight round it, so a
    /// unit in it can cross into any of them without the move step having
    /// to allocate. The spawn pass's job; cheap when they are there.
    pub fn prepare(&mut self, around: ChunkPos) {
        for pos in around.around(1) {
            if !self.rolls.contains(pos) {
                self.rolls.get_or_insert_with(pos, || Roll::EMPTY);
            }
        }
    }

    /// Drop empty lists with nobody within `radius` chunks, looking at no
    /// more than `count` of them from `cursor` on
    /// ([`crate::map::ChunkGrid::sweep`]); returns how many went. A list
    /// somebody is on is never dropped.
    pub fn sweep(&mut self, cursor: &mut usize, count: usize, radius: i32) -> usize {
        let held = self.rolls.len();
        if held == 0 {
            return 0;
        }
        let start = *cursor % held;
        let mut doomed = std::mem::take(&mut self.doomed);
        doomed.clear();
        doomed.extend(
            self.rolls
                .iter()
                .skip(start)
                .chain(self.rolls.iter().take(start))
                .take(count)
                .filter(|&(pos, roll)| roll.len == 0 && !self.anyone_within(pos, radius))
                .map(|(pos, _)| pos),
        );
        for &pos in &doomed {
            self.rolls.remove(pos);
        }
        let dropped = doomed.len();
        self.doomed = doomed;
        *cursor = start + count.min(held) - dropped;
        dropped
    }
}

impl std::fmt::Debug for Census {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Census")
            .field("lists", &self.rolls.len())
            .field("occupied", &self.occupied().count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots_in(census: &Census, pos: ChunkPos) -> Vec<Slot> {
        let mut slots: Vec<Slot> = census.units_in(pos).collect();
        slots.sort();
        slots
    }

    #[test]
    fn everybody_is_filed_in_the_chunk_they_stand_in() {
        let mut census = Census::new();
        census.file(0, Point::new(1, 1));
        census.file(1, Point::new(31, 0));
        census.file(2, Point::new(32, 0));
        census.file(5, Point::new(-1, -1));
        assert_eq!(slots_in(&census, ChunkPos::new(0, 0)), [0, 1]);
        assert_eq!(slots_in(&census, ChunkPos::new(1, 0)), [2]);
        assert_eq!(slots_in(&census, ChunkPos::new(-1, -1)), [5]);
        assert_eq!(census.count(ChunkPos::new(0, 0)), 2);
        assert_eq!(census.chunk_of(5), Some(ChunkPos::new(-1, -1)));
        assert_eq!(census.chunk_of(3), None, "never filed");
        let mut occupied: Vec<ChunkPos> = census.occupied().collect();
        occupied.sort();
        assert_eq!(occupied, [ChunkPos::new(-1, -1), ChunkPos::new(0, 0), ChunkPos::new(1, 0)]);
    }

    #[test]
    fn a_stack_in_one_cell_is_everybody_in_it_not_just_the_first() {
        // What the occupancy layer cannot say: it names one per cell.
        let mut census = Census::new();
        for slot in 0..4 {
            census.file(slot, Point::new(7, 7));
        }
        assert_eq!(slots_in(&census, ChunkPos::new(0, 0)), [0, 1, 2, 3]);
    }

    #[test]
    fn crossing_into_another_chunk_moves_somebody_and_nobody_else() {
        let mut census = Census::new();
        for slot in 0..3 {
            census.file(slot, Point::new(30, 5));
        }
        census.moved(1, Point::new(31, 5));
        assert_eq!(census.count(ChunkPos::new(0, 0)), 3, "still in the same chunk");
        census.moved(1, Point::new(32, 5));
        assert_eq!(slots_in(&census, ChunkPos::new(0, 0)), [0, 2]);
        assert_eq!(slots_in(&census, ChunkPos::new(1, 0)), [1]);
        // That was out of the middle of a list; now its head, then its last.
        for (slot, x) in [(2, 64), (0, 64)] {
            census.moved(slot, Point::new(x, 5));
        }
        assert_eq!(census.count(ChunkPos::new(0, 0)), 0);
        assert_eq!(slots_in(&census, ChunkPos::new(2, 0)), [0, 2]);
        census.strike(2);
        census.strike(2);
        assert_eq!(slots_in(&census, ChunkPos::new(2, 0)), [0]);
        census.moved(2, Point::new(0, 0));
        assert_eq!(census.count(ChunkPos::new(0, 0)), 0, "struck off, so not moved back on");
    }

    #[test]
    fn a_slot_used_again_is_filed_afresh() {
        let mut census = Census::new();
        census.file(0, Point::new(1, 1));
        census.strike(0);
        census.file(0, Point::new(100, 1));
        assert_eq!(census.count(ChunkPos::new(0, 0)), 0);
        assert_eq!(slots_in(&census, ChunkPos::new(3, 0)), [0]);
    }

    #[test]
    fn preparing_makes_the_lists_round_a_chunk_and_a_crossing_allocates_none() {
        let mut census = Census::new();
        census.file(0, Point::new(-40, 10));
        census.prepare(ChunkPos::of(Point::new(-40, 10)));
        assert_eq!(census.lists(), 9);
        // From chunk (-2, 0) diagonally into (-3, 1).
        census.moved(0, Point::new(-70, 40));
        assert_eq!(census.lists(), 9, "the list it stepped into was there");
        assert_eq!(census.chunk_of(0), Some(ChunkPos::new(-3, 1)));
        assert!(census.anyone_within(ChunkPos::new(-2, 2), 1));
        assert!(!census.anyone_within(ChunkPos::new(-1, 1), 1));
    }

    #[test]
    fn a_sweep_drops_empty_lists_only_once_nobody_is_near() {
        let mut census = Census::new();
        census.file(0, Point::new(0, 0));
        for x in 0..6 {
            census.prepare(ChunkPos::new(x, 0));
        }
        let lists = census.lists();
        let mut cursor = 0;
        let mut dropped = 0;
        for _ in 0..10 {
            dropped += census.sweep(&mut cursor, 5, 2);
        }
        // Chunk (0, 0) and everything within two of it stays.
        assert_eq!(census.lists(), lists - dropped);
        assert!(census.rolls.iter().all(|(pos, _)| census.anyone_within(pos, 2)));
        assert!(dropped > 0);
        census.strike(0);
        for _ in 0..10 {
            census.sweep(&mut cursor, 5, 2);
        }
        assert_eq!(census.lists(), 0);
    }
}
