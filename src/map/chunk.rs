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
//! [`ChunkGrid`] is the container: chunks keyed by [`ChunkPos`], in a flat
//! grid of slots that grows to fit whatever is loaded, so finding one is an
//! offset and never a hash. A slot is a pointer, so a sparse grid — a camera
//! that flew a long way out and back — costs eight bytes a chunk it skipped,
//! not a chunk's worth.

use super::coords::{Bounds, Point, Size};
use super::terrain::TerrainId;

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
/// [`super::PassabilityMap`] is for the site.
#[derive(Clone)]
pub struct Chunk {
    tiles: [TerrainId; CHUNK_AREA],
    passable: [u64; CHUNK_AREA / 64],
    see_through: [u64; CHUNK_AREA / 64],
}

impl Chunk {
    /// The chunk at `pos`, every cell asked of `ground`.
    pub fn generate(pos: ChunkPos, ground: impl Fn(Point) -> TerrainId) -> Box<Chunk> {
        let mut chunk = Box::new(Chunk {
            tiles: [TerrainId(0); CHUNK_AREA],
            passable: [0; CHUNK_AREA / 64],
            see_through: [0; CHUNK_AREA / 64],
        });
        let origin = pos.origin();
        for index in 0..CHUNK_AREA {
            let cell = origin.offset(index as i32 % CHUNK, index as i32 / CHUNK);
            let tile = ground(cell);
            chunk.tiles[index] = tile;
            if tile.is_passable() {
                chunk.passable[index / 64] |= 1 << (index % 64);
            }
            if tile.is_see_through() {
                chunk.see_through[index / 64] |= 1 << (index % 64);
            }
        }
        chunk
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

/// Chunks by where they are, in a grid of slots that grows to fit.
///
/// The slots cover a rectangle of chunk positions; a chunk outside it makes
/// the rectangle bigger (with room to spare, so a camera panning one way does
/// not re-lay the grid every chunk it crosses). Re-laying moves pointers,
/// never chunks.
pub struct ChunkGrid<T> {
    /// The chunk position slot 0 is for.
    min: ChunkPos,
    /// In chunks. Zero while nothing has ever been loaded.
    width: i32,
    height: i32,
    slots: Vec<Option<Box<T>>>,
    len: usize,
}

impl<T> Default for ChunkGrid<T> {
    fn default() -> Self {
        ChunkGrid { min: ChunkPos::default(), width: 0, height: 0, slots: Vec::new(), len: 0 }
    }
}

impl<T: Clone> Clone for ChunkGrid<T> {
    fn clone(&self) -> Self {
        ChunkGrid {
            min: self.min,
            width: self.width,
            height: self.height,
            slots: self.slots.clone(),
            len: self.len,
        }
    }
}

impl<T> ChunkGrid<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many chunks are loaded.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn slot(&self, pos: ChunkPos) -> Option<usize> {
        let (dx, dy) = (pos.x.wrapping_sub(self.min.x), pos.y.wrapping_sub(self.min.y));
        (dx >= 0 && dy >= 0 && dx < self.width && dy < self.height)
            .then(|| (dy as usize) * self.width as usize + dx as usize)
    }

    #[inline]
    pub fn get(&self, pos: ChunkPos) -> Option<&T> {
        self.slots[self.slot(pos)?].as_deref()
    }

    pub fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut T> {
        let slot = self.slot(pos)?;
        self.slots[slot].as_deref_mut()
    }

    /// The chunk holding `cell`, if it is loaded.
    #[inline]
    pub fn at(&self, cell: Point) -> Option<&T> {
        self.get(ChunkPos::of(cell))
    }

    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.get(pos).is_some()
    }

    /// The chunk at `pos`, made by `make` if it is not loaded yet.
    pub fn get_or_insert_with(&mut self, pos: ChunkPos, make: impl FnOnce() -> Box<T>) -> &mut T {
        self.reach(pos);
        let slot = self.slot(pos).expect("reach grew the grid to it");
        if self.slots[slot].is_none() {
            self.slots[slot] = Some(make());
            self.len += 1;
        }
        self.slots[slot].as_deref_mut().expect("just filled")
    }

    /// Drop the chunk at `pos`; returns whether there was one.
    pub fn remove(&mut self, pos: ChunkPos) -> bool {
        let Some(slot) = self.slot(pos) else {
            return false;
        };
        let removed = self.slots[slot].take().is_some();
        self.len -= usize::from(removed);
        removed
    }

    /// Keep only the chunks `keep` says yes to; returns how many went.
    pub fn retain(&mut self, mut keep: impl FnMut(ChunkPos) -> bool) -> usize {
        let mut dropped = 0;
        for (slot, chunk) in self.slots.iter_mut().enumerate() {
            if chunk.is_some() {
                let pos = ChunkPos::new(
                    self.min.x + (slot % self.width as usize) as i32,
                    self.min.y + (slot / self.width as usize) as i32,
                );
                if !keep(pos) {
                    *chunk = None;
                    dropped += 1;
                }
            }
        }
        self.len -= dropped;
        dropped
    }

    /// Every loaded chunk, row by row.
    pub fn iter(&self) -> impl Iterator<Item = (ChunkPos, &T)> {
        let (min, width) = (self.min, self.width.max(1) as usize);
        self.slots.iter().enumerate().filter_map(move |(slot, chunk)| {
            let pos = ChunkPos::new(min.x + (slot % width) as i32, min.y + (slot / width) as i32);
            chunk.as_deref().map(|chunk| (pos, chunk))
        })
    }

    /// Every loaded chunk, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots.iter_mut().filter_map(|chunk| chunk.as_deref_mut())
    }

    /// Grow the slot rectangle to take in `pos`, with half as much again
    /// spare on the side it grew, so a steady pan re-lays it rarely.
    fn reach(&mut self, pos: ChunkPos) {
        if self.slot(pos).is_some() {
            return;
        }
        if self.width == 0 {
            self.min = ChunkPos::new(pos.x - 2, pos.y - 2);
            self.width = 5;
            self.height = 5;
            self.slots = (0..25).map(|_| None).collect();
            return;
        }
        let spare_x = (self.width / 2).max(2);
        let spare_y = (self.height / 2).max(2);
        let mut min = self.min;
        let mut max = ChunkPos::new(self.min.x + self.width - 1, self.min.y + self.height - 1);
        if pos.x < min.x {
            min.x = pos.x - spare_x;
        }
        if pos.y < min.y {
            min.y = pos.y - spare_y;
        }
        if pos.x > max.x {
            max.x = pos.x + spare_x;
        }
        if pos.y > max.y {
            max.y = pos.y + spare_y;
        }
        let (width, height) = (max.x - min.x + 1, max.y - min.y + 1);
        let mut slots: Vec<Option<Box<T>>> = (0..width as usize * height as usize).map(|_| None).collect();
        for (slot, chunk) in self.slots.drain(..).enumerate() {
            if let Some(chunk) = chunk {
                let x = self.min.x + (slot % self.width as usize) as i32 - min.x;
                let y = self.min.y + (slot / self.width as usize) as i32 - min.y;
                slots[y as usize * width as usize + x as usize] = Some(chunk);
            }
        }
        self.min = min;
        self.width = width;
        self.height = height;
        self.slots = slots;
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
            *grid.get_or_insert_with(pos, || Box::new(n as i32)) += 0;
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
            grid.get_or_insert_with(ChunkPos::new(x, 0), || Box::new(0));
        }
        assert_eq!(grid.retain(|pos| pos.x >= 0), 3);
        assert_eq!(grid.len(), 3);
        assert!(grid.remove(ChunkPos::new(1, 0)));
        assert!(!grid.remove(ChunkPos::new(1, 0)));
        let left: Vec<ChunkPos> = grid.iter().map(|(pos, _)| pos).collect();
        assert_eq!(left, [ChunkPos::new(0, 0), ChunkPos::new(2, 0)]);
    }

    #[test]
    fn a_generated_chunk_derives_its_bits_from_its_tiles() {
        use super::super::terrain::{DIRT, WATER};
        let pos = ChunkPos::new(-1, 2);
        let ground = |cell: Point| if (cell.x + cell.y) % 3 == 0 { WATER } else { DIRT };
        let chunk = Chunk::generate(pos, ground);
        for cell in pos.bounds().points() {
            assert_eq!(chunk.tile(cell), ground(cell));
            assert_eq!(chunk.is_passable(cell), ground(cell).is_passable());
            assert!(chunk.is_see_through(cell));
        }
    }
}
