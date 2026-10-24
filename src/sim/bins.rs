//! What the stores hold: [`Bins`].
//!
//! A store is a feature things are kept in, each kind holding one
//! [`ItemKind`] — a row of [`FeatureKind`]'s catalogue with a
//! [`StoreDef`](super::feature::StoreDef): the warehouse's **scrap bins**,
//! where a scrap collector stores what it brings in from the wasteland
//! (`brain::goals::CollectGoal`), and its **steel rack**; the steel factory's
//! **scrap hopper** and **steel pallet**, the scrap waiting to be smelted and
//! the steel that came of it. Couriers carry scrap from the bins to the
//! hoppers and steel from the pallets to the rack (`brain::goals::HaulGoal`);
//! steelworkers take scrap out of a hopper and put steel on a pallet
//! (`brain::goals::SmeltGoal`).
//!
//! Like a fridge's door, how much a store holds is state the world owns and
//! no one unit does, so a unit's task *asks* for its pieces to be put in
//! ([`Effect::Deposit`](super::Effect)) or taken out
//! ([`Effect::Withdraw`](super::Effect)), and the world step does it
//! ([`crate::sim::world_step`]), sequentially, in slot order — so two units
//! at the last piece get one between them.
//!
//! **A store never fills.** It holds whatever is put in it.

use crate::map::{Map, ObjectLayer, Point};
use crate::save::{invalid, pb, Invalid};

use super::feature::{self, FeatureKind};
use super::item::ItemKind;

/// Where the stores are, what kind each is, and how many pieces each holds.
///
/// Cells are found once, in [`GameState::new`](crate::sim::GameState::new),
/// the way [`crate::sim::Fridges`] are; what is stored changes every time
/// somebody empties a pack into one or fills a pack from one. Kept sorted, so
/// a lookup is a binary search and nothing depends on the order the stores
/// were painted in.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Bins {
    cells: Vec<Point>,
    kinds: Vec<FeatureKind>,
    stored: Vec<u32>,
}

/// No stores anywhere: what a world without a [`super::GameState`] — a
/// test's — hands its units to look through.
pub static NONE: Bins = Bins::new();

impl Bins {
    pub const fn new() -> Bins {
        Bins { cells: Vec::new(), kinds: Vec::new(), stored: Vec::new() }
    }

    /// Every store on `map`, filled with its authored new-game supplies.
    pub fn from_map(map: &Map) -> Bins {
        let mut found: Vec<(Point, FeatureKind)> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter_map(|object| Some((object.cell(), feature::store_of(object.kind.as_str())?)))
            .collect();
        found.sort_unstable_by_key(|&(cell, _)| cell);
        found.dedup_by_key(|&mut (cell, _)| cell);
        let stored = vec![0; found.len()];
        let (cells, kinds) = found.into_iter().unzip();
        let mut bins = Bins { cells, kinds, stored };
        for stock in map.initial_stock() {
            let _ = bins.add(stock.cell, stock.pieces);
        }
        bins
    }

    fn index(&self, cell: Point) -> Option<usize> {
        self.cells.binary_search(&cell).ok()
    }

    /// Whether there is a store in `cell`.
    pub fn has(&self, cell: Point) -> bool {
        self.index(cell).is_some()
    }

    /// The kind of store in `cell`, if any.
    pub fn store(&self, cell: Point) -> Option<FeatureKind> {
        self.index(cell).map(|i| self.kinds[i])
    }

    /// What the store in `cell` holds, if there is one.
    pub fn item(&self, cell: Point) -> Option<ItemKind> {
        Some(self.store(cell)?.store()?.holds)
    }

    /// How many pieces the store in `cell` holds; `0` where there is none.
    pub fn stored(&self, cell: Point) -> u32 {
        self.index(cell).map_or(0, |i| self.stored[i])
    }

    /// How many pieces every store holds, together.
    pub fn total(&self) -> u32 {
        self.stored.iter().sum()
    }

    /// How many pieces every store of `kind` holds, together.
    pub fn total_in(&self, kind: FeatureKind) -> u32 {
        self.iter_stores().filter(|&(_, store, _)| store == kind).map(|(_, _, stored)| stored).sum()
    }

    /// How many stores of `kind` there are.
    pub fn count(&self, kind: FeatureKind) -> usize {
        self.kinds.iter().filter(|&&store| store == kind).count()
    }

    /// How many stores there are.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Every store, cell and what it holds, sorted by cell.
    pub fn iter(&self) -> impl Iterator<Item = (Point, u32)> + '_ {
        self.cells.iter().copied().zip(self.stored.iter().copied())
    }

    /// Every store, cell, kind and what it holds, sorted by cell.
    pub fn iter_stores(&self) -> impl Iterator<Item = (Point, FeatureKind, u32)> + '_ {
        self.cells.iter().zip(&self.kinds).zip(&self.stored).map(|((&cell, &store), &stored)| (cell, store, stored))
    }

    /// Put `pieces` in the store in `cell`, and say how many it holds now.
    /// `None`, and nothing stored, where there is no store.
    #[must_use = "a cell with no store in it refuses this"]
    pub fn add(&mut self, cell: Point, pieces: u32) -> Option<u32> {
        let i = self.index(cell)?;
        self.stored[i] = self.stored[i].saturating_add(pieces);
        Some(self.stored[i])
    }

    /// Take one piece out of the store in `cell`. `false`, and nothing
    /// taken, from an empty store or where there is none.
    #[must_use = "an empty store refuses this"]
    pub fn take(&mut self, cell: Point) -> bool {
        match self.index(cell) {
            Some(i) if self.stored[i] > 0 => {
                self.stored[i] -= 1;
                true
            }
            _ => false,
        }
    }

    // --- saving ---

    /// What every store holds. Which kind each is, is the map's.
    pub(crate) fn save(&self) -> Vec<pb::Bin> {
        let Bins { cells, kinds: _, stored } = self;
        cells
            .iter()
            .zip(stored)
            .map(|(&cell, &stored)| pb::Bin { cell: Some(cell.into()), stored })
            .collect()
    }

    /// Put back what every store holds, into stores found on the same map:
    /// the same stores, in the same cells, or the save is not of this map.
    pub(crate) fn restore(&mut self, saved: &[pb::Bin]) -> Result<(), Invalid> {
        if saved.len() != self.cells.len() {
            return invalid(format!("{} stores saved for a map with {}", saved.len(), self.cells.len()));
        }
        for bin in saved {
            let cell = pb::cell(bin.cell, "store")?;
            let Some(i) = self.index(cell) else {
                return invalid(format!("no store at {}, {}", cell.x, cell.y));
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
        prop_at(&mut map, "scrap bin", a);
        prop_at(&mut map, "scrap bin", b);
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

    /// Every kind of store is found by its prop, holds its one item, and
    /// gives pieces back one at a time until it is empty.
    #[test]
    fn every_store_holds_its_own_item_and_gives_it_back_until_it_is_empty() {
        let mut map = Map::new(Size::new(8, 8), FLOOR);
        let stores = [FeatureKind::ScrapBin, FeatureKind::SteelRack, FeatureKind::ScrapHopper, FeatureKind::SteelPallet];
        let cells = [Point::new(1, 1), Point::new(3, 1), Point::new(5, 1), Point::new(1, 5)];
        for (store, cell) in stores.into_iter().zip(cells) {
            prop_at(&mut map, store.prop(), cell);
        }
        let mut bins = Bins::from_map(&map);
        assert_eq!(bins.len(), 4);
        for (store, cell) in stores.into_iter().zip(cells) {
            assert_eq!(bins.store(cell), Some(store));
            assert_eq!(bins.count(store), 1);
        }
        assert_eq!(bins.item(cells[0]), Some(ItemKind::Scrap), "a bin holds scrap");
        assert_eq!(bins.item(cells[1]), Some(ItemKind::Steel), "a rack steel");
        assert_eq!(bins.item(cells[2]), Some(ItemKind::Scrap), "a hopper scrap");
        assert_eq!(bins.item(cells[3]), Some(ItemKind::Steel), "a pallet steel");
        let site = |kind: FeatureKind| kind.store().unwrap().site;
        assert_eq!(stores.map(site), ["warehouse", "warehouse", "factory", "factory"]);

        let _ = bins.add(cells[3], 2);
        assert_eq!(bins.total_in(FeatureKind::SteelPallet), 2);
        assert!(bins.take(cells[3]) && bins.take(cells[3]));
        assert!(!bins.take(cells[3]), "empty");
        assert!(!bins.take(Point::new(7, 7)), "no store there");
        assert_eq!(bins.total(), 0);
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
