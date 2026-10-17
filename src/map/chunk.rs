//! Chunks: the endless world, a square at a time.
//!
//! The world has no edge (the wasteland, [`super::wasteland`], goes on in
//! every direction), so nothing can be one array laid over all of it the way
//! the authored map's layers are laid over the site. It is cut instead into
//! [`CHUNK`]-cell squares, and a square exists in memory only once something
//! has needed it — a camera has looked at it, a unit has walked near it.
//! That is what *loaded* means here, and it is all it means: what a chunk
//! holds is a pure function of the world's seed and where the chunk is, so
//! loading one never changes what any cell is. Reading a cell whose chunk is
//! not loaded gives the same answer, only slower (the generator is asked).
//! That is what keeps loading out of the simulation's results: the camera can
//! decide what is loaded without deciding anything about the world.
//!
//! [`ChunkGrid`] is the container: chunks keyed by [`ChunkPos`], sparse, so
//! what it costs follows how many chunks are loaded and never how far apart
//! they are — a camera fifty thousand cells out and a crowd scattered to the
//! four winds cost what their chunks cost, and dropping a chunk gives its
//! room back.

use super::coords::{Bounds, Point, Size};
use super::terrain::TerrainId;
use super::wasteland::Scrap;

/// Cells along one side of a chunk.
///
/// Thirty-two: a chunk is a thousand cells, a couple of kilobytes of ground,
/// and a word of bits is two rows of it. A default-zoom canvas is under seven
/// cells by four, so one to four chunks are on it; zoomed right out, a few
/// dozen.
pub const CHUNK: i32 = 32;

/// Cells in a chunk.
pub const CHUNK_AREA: usize = (CHUNK * CHUNK) as usize;

/// Where a chunk is, in chunks: chunk `(0, 0)` holds cells `(0, 0)` to
/// `(31, 31)`, chunk `(-1, 0)` cells `(-32, 0)` to `(-1, 31)`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ChunkPos {
    pub x: i32,
    pub y: i32,
}

impl ChunkPos {
    pub const fn new(x: i32, y: i32) -> ChunkPos {
        ChunkPos { x, y }
    }

    /// The chunk a cell is in. Floored, so cell `-1` is in chunk `-1`.
    pub const fn of(cell: Point) -> ChunkPos {
        ChunkPos::new(cell.x.div_euclid(CHUNK), cell.y.div_euclid(CHUNK))
    }

    /// Its bottom-left cell.
    pub const fn origin(self) -> Point {
        Point::new(self.x * CHUNK, self.y * CHUNK)
    }

    /// The cells it holds.
    pub const fn bounds(self) -> Bounds {
        Bounds::new(self.origin(), Size { width: CHUNK, height: CHUNK })
    }

    /// Every chunk with a cell in `bounds`, row by row.
    pub fn covering(bounds: Bounds) -> impl Iterator<Item = ChunkPos> {
        let (min, max) = (ChunkPos::of(bounds.origin), ChunkPos::of(bounds.max()));
        (min.y..=max.y).flat_map(move |y| (min.x..=max.x).map(move |x| ChunkPos::new(x, y)))
    }

    /// The chunks within `radius` chunks of this one, itself included.
    pub fn around(self, radius: i32) -> impl Iterator<Item = ChunkPos> {
        (self.y - radius..=self.y + radius)
            .flat_map(move |y| (self.x - radius..=self.x + radius).map(move |x| ChunkPos::new(x, y)))
    }
}

/// Where a cell is inside its chunk: a row-major index from 0 to
/// [`CHUNK_AREA`].
pub const fn local_index(cell: Point) -> usize {
    (cell.y.rem_euclid(CHUNK) * CHUNK + cell.x.rem_euclid(CHUNK)) as usize
}

/// One chunk of ground: a tile per cell, and the two bits the simulation
/// reads most — whether a cell can be walked and whether it can be seen
/// through — worked out once when it is made, as the map's own
/// [`super::PassabilityMap`] is for the site. And the heaps of scrap standing
/// on it ([`super::wasteland`]), which are in the way: a heap's cell is not
/// passable until it is cleared ([`Chunk::clear`]).
#[derive(Clone)]
pub struct Chunk {
    tiles: [TerrainId; CHUNK_AREA],
    passable: [u64; CHUNK_AREA / 64],
    see_through: [u64; CHUNK_AREA / 64],
    /// Every heap of scrap in the chunk as it was found, by local index,
    /// ascending. Empty in nearly every chunk; a few dozen in a scrapyard.
    scrap: Vec<(u16, Scrap)>,
}

impl Chunk {
    /// The chunk at `pos`, every cell asked of `ground`, and every cell that
    /// can be walked asked of `scrap` whether a heap stands on that ground.
    pub fn generate(
        pos: ChunkPos,
        ground: impl Fn(Point) -> TerrainId,
        scrap: impl Fn(Point, TerrainId) -> Option<Scrap>,
    ) -> Box<Chunk> {
        let mut chunk = Box::new(Chunk {
            tiles: [TerrainId(0); CHUNK_AREA],
            passable: [0; CHUNK_AREA / 64],
            see_through: [0; CHUNK_AREA / 64],
            scrap: Vec::new(),
        });
        let origin = pos.origin();
        for index in 0..CHUNK_AREA {
            let cell = origin.offset(index as i32 % CHUNK, index as i32 / CHUNK);
            let tile = ground(cell);
            chunk.tiles[index] = tile;
            if tile.is_passable() {
                match scrap(cell, tile) {
                    Some(heap) => chunk.scrap.push((index as u16, heap)),
                    None => chunk.passable[index / 64] |= 1 << (index % 64),
                }
            }
            if tile.is_see_through() {
                chunk.see_through[index / 64] |= 1 << (index % 64);
            }
        }
        chunk
    }

    /// The heap of scrap in a cell of this chunk as it was found — whatever
    /// has been taken from it since, which the map keeps
    /// (`Map::scrap`). A binary search of the chunk's few heaps.
    #[inline]
    pub fn scrap(&self, cell: Point) -> Option<Scrap> {
        let index = local_index(cell) as u16;
        self.scrap
            .binary_search_by_key(&index, |&(at, _)| at)
            .ok()
            .map(|found| self.scrap[found].1)
    }

    /// Every heap in the chunk as it was found, bottom row first.
    pub fn heaps(&self, pos: ChunkPos) -> impl Iterator<Item = (Point, Scrap)> + '_ {
        let origin = pos.origin();
        self.scrap
            .iter()
            .map(move |&(index, heap)| (origin.offset(index as i32 % CHUNK, index as i32 / CHUNK), heap))
    }

    /// The heap in `cell` is gone: the cell is as walkable as its ground.
    /// The heap stays listed, as found; what is left of it is the map's to
    /// say.
    pub fn clear(&mut self, cell: Point) {
        let index = local_index(cell);
        if self.tiles[index].is_passable() {
            self.passable[index / 64] |= 1 << (index % 64);
        }
    }

    /// The tile of a cell in this chunk. A cell of another chunk reads the
    /// cell in the same place in this one — the caller found the chunk.
    #[inline]
    pub fn tile(&self, cell: Point) -> TerrainId {
        self.tiles[local_index(cell)]
    }

    #[inline]
    pub fn is_passable(&self, cell: Point) -> bool {
        let index = local_index(cell);
        self.passable[index / 64] & (1 << (index % 64)) != 0
    }

    #[inline]
    pub fn is_see_through(&self, cell: Point) -> bool {
        let index = local_index(cell);
        self.see_through[index / 64] & (1 << (index % 64)) != 0
    }
}

/// Chunks by where they are: a hash index over a dense list.
///
/// **Sparse.** It used to be a rectangle of slots grown to take in every
/// chunk ever loaded, which never shrank: two units walking off in opposite
/// directions made it the size of the distance between them. Now memory is
/// a few words per chunk *held*, wherever those chunks are, and dropping
/// chunks gives the room back (the index shrinks when it falls well below a
/// quarter full).
///
/// The values sit in one `Vec` in the order they arrived; a removal moves the
/// last one into the gap. That is what [`ChunkGrid::iter`] and
/// [`ChunkGrid::sweep`] walk. Beside it is the index, an open-addressed table
/// with linear probing, at most half full: a slot holds a position and where
/// its value is in the list, so a lookup is a multiply, a shift and usually a
/// single compare in a table that fits in cache, and touches no value but the
/// one asked for. The hash is this file's own (Fibonacci hashing of the
/// position), not the standard library's seeded one, so the same history lays
/// the table out the same way on every machine and every run — iteration
/// order included, which a simulation needs if anything ever depends on it.
///
/// A value is stored inline: a big one (a chunk of ground, a chunk of
/// occupancy) is a `Box` so that growing the list moves pointers, a small one
/// (a unit list's head) is not boxed at all.
#[derive(Clone)]
pub struct ChunkGrid<T> {
    /// A power of two long, or empty while nothing has ever been held.
    index: Vec<IndexSlot>,
    /// `64 - log2(index.len())`: Fibonacci hashing keeps the top bits.
    shift: u32,
    values: Vec<(ChunkPos, T)>,
}

/// One slot of the index: a position, and where its value is.
#[derive(Clone, Copy, Debug)]
struct IndexSlot {
    pos: ChunkPos,
    /// Into `values`; [`VACANT`] for a slot with nothing in it.
    at: u32,
}

const VACANT: u32 = u32::MAX;

impl IndexSlot {
    const EMPTY: IndexSlot = IndexSlot { pos: ChunkPos::new(0, 0), at: VACANT };
}

/// The smallest index made, in slots.
const MIN_INDEX: usize = 16;

impl<T> Default for ChunkGrid<T> {
    fn default() -> Self {
        ChunkGrid { index: Vec::new(), shift: 64, values: Vec::new() }
    }
}

impl<T> ChunkGrid<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many chunks are held.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Slots in the index — what the grid costs beyond its values, for a test
    /// that the cost follows what is held.
    pub fn index_len(&self) -> usize {
        self.index.len()
    }

    /// Where `pos` would start looking in the index.
    #[inline]
    fn home(&self, pos: ChunkPos) -> usize {
        let key = ((pos.x as u32 as u64) << 32) | pos.y as u32 as u64;
        (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> self.shift) as usize
    }

    /// The index slot holding `pos`, if it is held.
    #[inline]
    fn probe(&self, pos: ChunkPos) -> Option<usize> {
        if self.index.is_empty() {
            return None;
        }
        let mask = self.index.len() - 1;
        let mut slot = self.home(pos);
        loop {
            let entry = self.index[slot];
            if entry.at == VACANT {
                return None;
            }
            if entry.pos == pos {
                return Some(slot);
            }
            slot = (slot + 1) & mask;
        }
    }

    #[inline]
    pub fn get(&self, pos: ChunkPos) -> Option<&T> {
        let slot = self.probe(pos)?;
        Some(&self.values[self.index[slot].at as usize].1)
    }

    pub fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut T> {
        let slot = self.probe(pos)?;
        Some(&mut self.values[self.index[slot].at as usize].1)
    }

    /// The chunk holding `cell`, if it is held.
    #[inline]
    pub fn at(&self, cell: Point) -> Option<&T> {
        self.get(ChunkPos::of(cell))
    }

    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.probe(pos).is_some()
    }

    /// The value at `pos`, made by `make` if there is none yet. Allocates
    /// only when the list or the index has to grow.
    pub fn get_or_insert_with(&mut self, pos: ChunkPos, make: impl FnOnce() -> T) -> &mut T {
        let at = match self.probe(pos) {
            Some(slot) => self.index[slot].at as usize,
            None => {
                if (self.values.len() + 1) * 2 > self.index.len() {
                    self.rehash((self.index.len() * 2).max(MIN_INDEX));
                }
                let at = self.values.len();
                self.place(pos, at as u32);
                self.values.push((pos, make()));
                at
            }
        };
        &mut self.values[at].1
    }

    /// Drop the value at `pos`, and hand it back.
    pub fn remove(&mut self, pos: ChunkPos) -> Option<T> {
        let slot = self.probe(pos)?;
        let value = self.take(slot);
        self.shrink_if_sparse();
        Some(value)
    }

    /// Keep only the values `keep` says yes to; returns how many went.
    pub fn retain(&mut self, mut keep: impl FnMut(ChunkPos, &T) -> bool) -> usize {
        let mut dropped = 0;
        let mut at = 0;
        while at < self.values.len() {
            let (pos, ref value) = self.values[at];
            if keep(pos, value) {
                at += 1;
            } else {
                // The last value moves into `at`, and is looked at next.
                let slot = self.probe(pos).expect("a held value is in the index");
                self.take(slot);
                dropped += 1;
            }
        }
        self.shrink_if_sparse();
        dropped
    }

    /// [`ChunkGrid::retain`] over at most `count` values, starting at
    /// `cursor` and leaving it where the next sweep should start: a bounded
    /// piece of a full pass, for a caller that wants to look at everything
    /// eventually and at a little each time. Returns how many went.
    pub fn sweep(&mut self, cursor: &mut usize, count: usize, mut keep: impl FnMut(ChunkPos, &T) -> bool) -> usize {
        let mut dropped = 0;
        for _ in 0..count {
            if self.values.is_empty() {
                break;
            }
            if *cursor >= self.values.len() {
                *cursor = 0;
            }
            let (pos, ref value) = self.values[*cursor];
            if keep(pos, value) {
                *cursor += 1;
            } else {
                let slot = self.probe(pos).expect("a held value is in the index");
                self.take(slot);
                dropped += 1;
            }
        }
        if dropped > 0 {
            self.shrink_if_sparse();
        }
        dropped
    }

    /// Every value with its position, in the order they arrived but for
    /// the gaps a removal filled.
    pub fn iter(&self) -> impl Iterator<Item = (ChunkPos, &T)> {
        self.values.iter().map(|(pos, value)| (*pos, value))
    }

    /// Every value with its position, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (ChunkPos, &mut T)> {
        self.values.iter_mut().map(|(pos, value)| (*pos, value))
    }

    /// Put `pos` in the index, pointing at `at`. There must be room.
    fn place(&mut self, pos: ChunkPos, at: u32) {
        let mask = self.index.len() - 1;
        let mut slot = self.home(pos);
        while self.index[slot].at != VACANT {
            slot = (slot + 1) & mask;
        }
        self.index[slot] = IndexSlot { pos, at };
    }

    /// Take out the value the index slot `slot` points at.
    ///
    /// The slot is emptied by shifting back whatever follows it in its run
    /// and would be unreachable past a gap — so no tombstones, and a probe
    /// still stops at the first vacant slot. Then the last value moves into
    /// the removed one's place in the list, and its index slot is told.
    fn take(&mut self, slot: usize) -> T {
        let mask = self.index.len() - 1;
        let at = self.index[slot].at as usize;
        let mut hole = slot;
        let mut next = (hole + 1) & mask;
        while self.index[next].at != VACANT {
            let home = self.home(self.index[next].pos);
            // The entry at `next` may fill the hole if its home is not
            // strictly between the hole and it, going round.
            if (next.wrapping_sub(home) & mask) >= (next.wrapping_sub(hole) & mask) {
                self.index[hole] = self.index[next];
                hole = next;
            }
            next = (next + 1) & mask;
        }
        self.index[hole] = IndexSlot::EMPTY;

        let (_, value) = self.values.swap_remove(at);
        if let Some(&(moved, _)) = self.values.get(at) {
            let slot = self.probe(moved).expect("the moved value is in the index");
            self.index[slot].at = at as u32;
        }
        value
    }

    /// Lay the index out again at `slots` long, a power of two.
    fn rehash(&mut self, slots: usize) {
        debug_assert!(slots.is_power_of_two() && slots >= self.values.len() * 2);
        self.index = vec![IndexSlot::EMPTY; slots];
        self.shift = 64 - slots.trailing_zeros();
        for at in 0..self.values.len() {
            let pos = self.values[at].0;
            self.place(pos, at as u32);
        }
    }

    /// Give room back once the grid holds far less than it is laid out for:
    /// an index under an eighth full is halved down to a quarter or so, and
    /// so is a list with four times the room it needs.
    fn shrink_if_sparse(&mut self) {
        let len = self.values.len();
        if self.index.len() > MIN_INDEX && len * 8 < self.index.len() {
            if len == 0 {
                *self = Self::default();
                return;
            }
            self.rehash((len * 4).next_power_of_two().max(MIN_INDEX));
        }
        if self.values.capacity() > 4 * len.max(MIN_INDEX) {
            self.values.shrink_to(2 * len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_is_in_the_chunk_its_coordinates_floor_to() {
        assert_eq!(ChunkPos::of(Point::new(0, 0)), ChunkPos::new(0, 0));
        assert_eq!(ChunkPos::of(Point::new(31, 31)), ChunkPos::new(0, 0));
        assert_eq!(ChunkPos::of(Point::new(32, 0)), ChunkPos::new(1, 0));
        assert_eq!(ChunkPos::of(Point::new(-1, -32)), ChunkPos::new(-1, -1));
        assert_eq!(ChunkPos::of(Point::new(-33, 5)), ChunkPos::new(-2, 0));
        for cell in [Point::new(-1, -1), Point::new(40, -70), Point::new(5, 5)] {
            assert!(ChunkPos::of(cell).bounds().contains(cell), "{cell:?}");
        }
    }

    #[test]
    fn every_cell_of_a_chunk_has_its_own_local_index() {
        let pos = ChunkPos::new(-3, 2);
        let mut seen = vec![false; CHUNK_AREA];
        for cell in pos.bounds().points() {
            let index = local_index(cell);
            assert!(!seen[index], "{cell:?}");
            seen[index] = true;
        }
        assert!(seen.into_iter().all(|seen| seen));
    }

    #[test]
    fn covering_finds_every_chunk_a_rectangle_touches() {
        let bounds = Bounds::from_corners(Point::new(-1, 0), Point::new(32, 31));
        let chunks: Vec<ChunkPos> = ChunkPos::covering(bounds).collect();
        assert_eq!(chunks, [ChunkPos::new(-1, 0), ChunkPos::new(0, 0), ChunkPos::new(1, 0)]);
    }

    #[test]
    fn the_grid_grows_in_every_direction_and_keeps_what_it_held() {
        let mut grid: ChunkGrid<i32> = ChunkGrid::new();
        let far = [
            ChunkPos::new(0, 0),
            ChunkPos::new(-40, 3),
            ChunkPos::new(17, -90),
            ChunkPos::new(3, 200),
        ];
        for (n, &pos) in far.iter().enumerate() {
            *grid.get_or_insert_with(pos, || n as i32) += 0;
        }
        assert_eq!(grid.len(), far.len());
        for (n, &pos) in far.iter().enumerate() {
            assert_eq!(grid.get(pos), Some(&(n as i32)), "{pos:?}");
        }
        assert_eq!(grid.get(ChunkPos::new(1, 1)), None);
        assert_eq!(grid.get(ChunkPos::new(i32::MAX, i32::MIN)), None);
        // Asking again makes nothing.
        grid.get_or_insert_with(far[1], || panic!("already loaded"));
    }

    #[test]
    fn retain_and_remove_unload() {
        let mut grid: ChunkGrid<u8> = ChunkGrid::new();
        for x in -3..3 {
            grid.get_or_insert_with(ChunkPos::new(x, 0), || 0);
        }
        assert_eq!(grid.retain(|pos, _| pos.x >= 0), 3);
        assert_eq!(grid.len(), 3);
        assert_eq!(grid.remove(ChunkPos::new(1, 0)), Some(0));
        assert_eq!(grid.remove(ChunkPos::new(1, 0)), None);
        let mut left: Vec<ChunkPos> = grid.iter().map(|(pos, _)| pos).collect();
        left.sort();
        assert_eq!(left, [ChunkPos::new(0, 0), ChunkPos::new(2, 0)]);
    }

    /// The bug this replaced: two chunks a world apart made a rectangle the
    /// size of the world between them. Now they cost two entries, and the
    /// index shrinks back once most of what it held is gone.
    #[test]
    fn what_the_grid_costs_follows_what_it_holds_not_how_far_apart() {
        let mut grid: ChunkGrid<u8> = ChunkGrid::new();
        let reach = crate::map::WORLD_REACH / CHUNK;
        grid.get_or_insert_with(ChunkPos::new(-reach, -reach), || 1);
        grid.get_or_insert_with(ChunkPos::new(reach, reach), || 2);
        assert_eq!(grid.index_len(), MIN_INDEX, "two chunks, the smallest index");

        for x in 0..1000 {
            grid.get_or_insert_with(ChunkPos::new(x * 7 - 3000, x % 13), || 3);
        }
        assert!(grid.index_len() >= 2 * grid.len());
        let grown = grid.index_len();
        assert_eq!(grid.retain(|pos, _| pos.x.abs() == reach), 1000);
        assert!(grid.index_len() < grown / 8, "{} slots for {} chunks", grid.index_len(), grid.len());
        assert_eq!(grid.get(ChunkPos::new(-reach, -reach)), Some(&1));
        assert_eq!(grid.get(ChunkPos::new(reach, reach)), Some(&2));
    }

    /// Every way in and out, checked against a map that is obviously right:
    /// a few thousand inserts, removals, sweeps and retains over a small
    /// area, so runs collide, wrap round the end of the index and are pulled
    /// back over gaps.
    #[test]
    fn the_grid_agrees_with_an_ordered_map_through_every_change() {
        use std::collections::BTreeMap;
        let mut grid: ChunkGrid<u32> = ChunkGrid::new();
        let mut model: BTreeMap<ChunkPos, u32> = BTreeMap::new();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut cursor = 0;
        for step in 0..20_000u32 {
            let roll = next();
            let pos = ChunkPos::new((roll % 23) as i32 - 11, ((roll >> 8) % 19) as i32 - 9);
            match (roll >> 16) % 10 {
                0..=4 => {
                    let held = *grid.get_or_insert_with(pos, || step);
                    assert_eq!(held, *model.entry(pos).or_insert(step), "{pos:?}");
                }
                5..=7 => assert_eq!(grid.remove(pos), model.remove(&pos), "{pos:?}"),
                8 => {
                    let keep = |pos: ChunkPos, value: &u32| (pos.x + pos.y + *value as i32) % 3 != 0;
                    let swept = grid.sweep(&mut cursor, 7, keep);
                    // A sweep drops some of what a full retain would, and
                    // nothing it would not.
                    let before = model.len();
                    model.retain(|&pos, value| {
                        let kept = grid.contains(pos);
                        assert!(kept || !keep(pos, value), "{pos:?} went for no reason");
                        kept
                    });
                    assert_eq!(before - model.len(), swept);
                }
                _ => {
                    let keep = |pos: ChunkPos, _: &u32| (pos.x * 5 + pos.y) % 4 != 0;
                    let dropped = grid.retain(keep);
                    let before = model.len();
                    model.retain(|&pos, value| keep(pos, value));
                    assert_eq!(before - model.len(), dropped);
                }
            }
            assert_eq!(grid.len(), model.len());
            if step % 97 == 0 {
                for x in -12..12 {
                    for y in -10..10 {
                        let pos = ChunkPos::new(x, y);
                        assert_eq!(grid.get(pos), model.get(&pos), "{pos:?} at step {step}");
                    }
                }
                let mut held: Vec<(ChunkPos, u32)> = grid.iter().map(|(pos, value)| (pos, *value)).collect();
                held.sort();
                assert_eq!(held, model.iter().map(|(pos, value)| (*pos, *value)).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn a_sweep_looks_at_a_little_at_a_time_and_comes_round_to_everything() {
        let mut grid: ChunkGrid<u8> = ChunkGrid::new();
        for x in 0..10 {
            grid.get_or_insert_with(ChunkPos::new(x, 0), || 0);
        }
        let mut cursor = 0;
        let mut seen = 0;
        assert_eq!(grid.sweep(&mut cursor, 4, |_, _| { seen += 1; true }), 0);
        assert_eq!((seen, cursor), (4, 4));
        // Dropping the odd ones a few at a time gets all five in the end.
        let mut dropped = 0;
        for _ in 0..10 {
            dropped += grid.sweep(&mut cursor, 3, |pos, _| pos.x % 2 == 0);
        }
        assert_eq!(dropped, 5);
        assert!(grid.iter().all(|(pos, _)| pos.x % 2 == 0));
    }

    #[test]
    fn a_generated_chunk_derives_its_bits_from_its_tiles() {
        use super::super::terrain::{DIRT, WATER};
        let pos = ChunkPos::new(-1, 2);
        let ground = |cell: Point| if (cell.x + cell.y) % 3 == 0 { WATER } else { DIRT };
        let chunk = Chunk::generate(pos, ground, |_, _| None);
        for cell in pos.bounds().points() {
            assert_eq!(chunk.tile(cell), ground(cell));
            assert_eq!(chunk.is_passable(cell), ground(cell).is_passable());
            assert!(chunk.is_see_through(cell));
        }
    }

    #[test]
    fn a_heap_of_scrap_blocks_its_cell_until_it_is_cleared() {
        use super::super::terrain::DIRT;
        let pos = ChunkPos::new(3, -2);
        let heap = |cell: Point, _| (cell.x % 5 == 0 && cell.y % 3 == 0).then_some(Scrap { variant: 1, pieces: 2 });
        let mut chunk = Chunk::generate(pos, |_| DIRT, heap);
        let heaps: Vec<(Point, Scrap)> = chunk.heaps(pos).collect();
        assert!(!heaps.is_empty());
        for cell in pos.bounds().points() {
            assert_eq!(chunk.scrap(cell), heap(cell, DIRT), "{cell:?}");
            assert_eq!(chunk.is_passable(cell), heap(cell, DIRT).is_none(), "{cell:?}");
            assert!(chunk.is_see_through(cell), "a heap is seen over");
        }
        assert_eq!(heaps.len(), pos.bounds().points().filter(|&cell| heap(cell, DIRT).is_some()).count());
        let (cell, _) = heaps[0];
        chunk.clear(cell);
        assert!(chunk.is_passable(cell));
        assert_eq!(chunk.scrap(cell), heap(cell, DIRT), "still listed as found");
    }
}
