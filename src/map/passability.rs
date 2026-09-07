//! The static passability map: which cells the crowd can walk through.
//!
//! "Static" as in *not the agents*. It answers only what the terrain says, so
//! it changes when the map is edited and not once per tick; anything dynamic —
//! who is standing where, a door someone is holding shut — belongs in a
//! separate layer that a query consults after this one.
//!
//! It exists as its own structure, derived from the terrain rather than read
//! through it, because it is the single hottest read in the simulation: every
//! step of every path expansion asks it about four cells. Going through the
//! terrain layer instead would mean an indirection into the catalogue and a
//! branch on the tile id for every one of those. Here the answer is one bit,
//! and a whole neighbourhood of a row is in one cache line.

use super::coords::{Point, Size};

/// One bit per cell, row-major, packed into 64-bit words.
#[derive(Clone)]
pub struct PassabilityMap {
    size: Size,
    words: Vec<u64>,
}

impl PassabilityMap {
    /// A map where nothing is passable yet.
    pub fn new(size: Size) -> Self {
        Self {
            size,
            words: vec![0; size.area().div_ceil(u64::BITS as usize)],
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Whether an agent can occupy `point`.
    ///
    /// Off the map is impassable, so callers do not need a bounds check of
    /// their own — a path expansion can just ask about all four neighbours of
    /// an edge cell and let the ones outside answer for themselves.
    pub fn is_passable(&self, point: Point) -> bool {
        match self.size.index_of(point) {
            Some(index) => self.words[index / 64] & (1 << (index % 64)) != 0,
            None => false,
        }
    }

    pub fn is_blocked(&self, point: Point) -> bool {
        !self.is_passable(point)
    }

    /// Set a single cell. Off-map writes are ignored.
    pub fn set(&mut self, point: Point, passable: bool) {
        let Some(index) = self.size.index_of(point) else {
            return;
        };
        let bit = 1 << (index % 64);
        if passable {
            self.words[index / 64] |= bit;
        } else {
            self.words[index / 64] &= !bit;
        }
    }

    /// The edge-sharing neighbours that can be walked into, in
    /// [`Point::CARDINALS`] order — the order a search must visit them in to
    /// stay deterministic.
    pub fn passable_neighbours(&self, point: Point) -> impl Iterator<Item = Point> + '_ {
        point
            .cardinal_neighbours()
            .into_iter()
            .filter(|&neighbour| self.is_passable(neighbour))
    }

    pub fn count_passable(&self) -> usize {
        self.words.iter().map(|word| word.count_ones() as usize).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_map_is_solid() {
        let map = PassabilityMap::new(Size::new(4, 4));
        assert!(map.size().points().all(|point| map.is_blocked(point)));
        assert_eq!(map.count_passable(), 0);
    }

    #[test]
    fn setting_one_cell_leaves_its_word_neighbours_alone() {
        // A cell shares its 64-bit word with 63 others; a bad mask would open
        // or close all of them, and on a map this wide that is most of it.
        let size = Size::new(20, 20);
        let mut map = PassabilityMap::new(size);
        let open = Point::new(7, 3);

        map.set(open, true);
        assert!(map.is_passable(open));
        assert_eq!(map.count_passable(), 1);

        map.set(open, false);
        assert_eq!(map.count_passable(), 0);
    }

    #[test]
    fn every_cell_is_addressed_by_its_own_bit() {
        let size = Size::new(9, 7);
        let mut map = PassabilityMap::new(size);
        for point in size.points() {
            map.set(point, true);
        }
        assert_eq!(map.count_passable(), size.area());
        for point in size.points() {
            assert!(map.is_passable(point), "{point:?}");
        }
    }

    #[test]
    fn cells_off_the_map_are_impassable() {
        let mut map = PassabilityMap::new(Size::new(3, 3));
        for point in map.size().points() {
            map.set(point, true);
        }
        for outside in [
            Point::new(-1, 1),
            Point::new(3, 1),
            Point::new(1, -1),
            Point::new(1, 3),
        ] {
            assert!(map.is_blocked(outside), "{outside:?}");
        }
        // ...and asking did not quietly widen the map.
        assert_eq!(map.count_passable(), 9);
    }

    #[test]
    fn only_open_neighbours_are_offered() {
        let mut map = PassabilityMap::new(Size::new(3, 3));
        let centre = Point::new(1, 1);
        map.set(centre, true);
        map.set(Point::new(2, 1), true);
        map.set(Point::new(1, 0), true);

        let neighbours: Vec<Point> = map.passable_neighbours(centre).collect();
        assert_eq!(neighbours, [Point::new(2, 1), Point::new(1, 0)]);

        // A corner's off-map neighbours drop out without a bounds check here.
        assert_eq!(map.passable_neighbours(Point::new(0, 0)).count(), 1);
    }
}
