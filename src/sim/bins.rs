//! What the scrap bins hold: [`Bins`].
//!
//! A bin is where a scrap collector stores what it brings in from the
//! wasteland (`sim::job`, `brain::goals::CollectGoal`): one to a collector, in
//! the district's warehouse. Like a fridge's door, how much a bin holds is
//! state the world owns and no one unit does, so a unit's
//! [`Deposit`](super::brain::tasks::Deposit) task *asks* for its pieces to be
//! put in ([`Effect::Deposit`](super::Effect)) and the world step puts them
//! in ([`crate::sim::world_step`]), sequentially, in slot order.
//!
//! **A bin never fills.** It holds whatever is put in it; nothing takes scrap
//! out of one yet.

use crate::map::{Map, ObjectLayer, Point};
use crate::save::{invalid, pb, Invalid};

/// The prop name a scrap bin is painted with — the only one
/// [`Bins::from_map`] looks for.
pub const SCRAP_BIN: &str = "scrap bin";

/// Where the bins are, and how many pieces of scrap metal each holds.
///
/// Cells are found once, in [`GameState::new`](crate::sim::GameState::new),
/// the way [`crate::sim::Fridges`] are; what is stored beside them changes
/// every time somebody empties a pack into one. Kept sorted, so a lookup is a
/// binary search and nothing depends on the order the bins were painted in.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Bins {
    cells: Vec<Point>,
    stored: Vec<u32>,
}

impl Bins {
    /// Every bin on `map`, each empty.
    pub fn from_map(map: &Map) -> Bins {
        let mut cells: Vec<Point> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|object| object.kind.as_str() == SCRAP_BIN)
            .map(|object| object.cell())
            .collect();
        cells.sort_unstable();
        cells.dedup();
        let stored = vec![0; cells.len()];
        Bins { cells, stored }
    }

    fn index(&self, cell: Point) -> Option<usize> {
        self.cells.binary_search(&cell).ok()
    }

    /// Whether there is a bin in `cell`.
    pub fn has(&self, cell: Point) -> bool {
        self.index(cell).is_some()
    }

    /// How many pieces the bin in `cell` holds; `0` where there is none.
    pub fn stored(&self, cell: Point) -> u32 {
        self.index(cell).map_or(0, |i| self.stored[i])
    }

    /// How many pieces every bin holds, together.
    pub fn total(&self) -> u32 {
        self.stored.iter().sum()
    }

    /// How many bins there are.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Every bin, cell and what it holds, sorted by cell.
    pub fn iter(&self) -> impl Iterator<Item = (Point, u32)> + '_ {
        self.cells.iter().copied().zip(self.stored.iter().copied())
    }

    /// Put `pieces` in the bin in `cell`, and say how many it holds now.
    /// `None`, and nothing stored, where there is no bin.
    #[must_use = "a cell with no bin in it refuses this"]
    pub fn add(&mut self, cell: Point, pieces: u32) -> Option<u32> {
        let i = self.index(cell)?;
        self.stored[i] = self.stored[i].saturating_add(pieces);
        Some(self.stored[i])
    }

    // --- saving ---

    /// What every bin holds.
    pub(crate) fn save(&self) -> Vec<pb::Bin> {
        let Bins { cells, stored } = self;
        cells
            .iter()
            .zip(stored)
            .map(|(&cell, &stored)| pb::Bin { cell: Some(cell.into()), stored })
            .collect()
    }

    /// Put back what every bin holds, into bins found on the same map: the
    /// same bins, in the same cells, or the save is not of this map.
    pub(crate) fn restore(&mut self, saved: &[pb::Bin]) -> Result<(), Invalid> {
        if saved.len() != self.cells.len() {
            return invalid(format!("{} scrap bins saved for a map with {}", saved.len(), self.cells.len()));
        }
        for bin in saved {
            let cell = pb::cell(bin.cell, "scrap bin")?;
            let Some(i) = self.index(cell) else {
                return invalid(format!("no scrap bin at {}, {}", cell.x, cell.y));
            };
            self.stored[i] = bin.stored;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR};
    use crate::sim::testing::prop_at;

    fn two_bins() -> (Bins, Point, Point) {
        let mut map = Map::new(Size::new(8, 8), FLOOR);
        let (a, b) = (Point::new(5, 1), Point::new(1, 6));
        prop_at(&mut map, SCRAP_BIN, a);
        prop_at(&mut map, SCRAP_BIN, b);
        prop_at(&mut map, "fridge", Point::new(3, 3));
        (Bins::from_map(&map), a, b)
    }

    #[test]
    fn every_bin_on_the_map_starts_empty() {
        let (bins, a, b) = two_bins();
        assert_eq!(bins.len(), 2, "the fridge is not a bin");
        assert!(bins.has(a) && bins.has(b));
        assert_eq!((bins.stored(a), bins.stored(b), bins.total()), (0, 0, 0));
        let mut sorted = [a, b];
        sorted.sort();
        assert_eq!(bins.iter().map(|(cell, _)| cell).collect::<Vec<_>>(), sorted, "sorted by cell");
    }

    #[test]
    fn what_is_put_in_a_bin_stays_in_it_and_counts_towards_the_total() {
        let (mut bins, a, b) = two_bins();
        assert_eq!(bins.add(a, 3), Some(3));
        assert_eq!(bins.add(a, 2), Some(5));
        assert_eq!(bins.add(b, 1), Some(1));
        assert_eq!((bins.stored(a), bins.stored(b), bins.total()), (5, 1, 6));
        assert_eq!(bins.add(Point::new(0, 0), 3), None, "no bin there");
        assert_eq!(bins.stored(Point::new(0, 0)), 0);
        assert_eq!(bins.total(), 6);
    }

    #[test]
    fn bins_come_back_from_a_save_and_one_of_another_map_is_refused() {
        let (mut bins, a, _) = two_bins();
        let _ = bins.add(a, 4);
        let mut loaded = two_bins().0;
        loaded.restore(&bins.save()).unwrap();
        assert_eq!(loaded, bins);

        let mut moved = bins.save();
        moved[0].cell = Some(Point::new(7, 7).into());
        assert!(two_bins().0.restore(&moved).is_err(), "no bin at 7, 7");
        let mut short = bins.save();
        short.pop();
        assert!(two_bins().0.restore(&short).is_err(), "a bin missing");
    }
}
