//! The store's **superfridge**: the district's food, which every fridge in
//! the world is stocked from — [`Superfridge`], run by the [`StoreRules`] of
//! `store.yaml`.
//!
//! **Nothing is carried to the fridges.** The superfridge is a store of meals
//! ([`FeatureKind::Superfridge`], kept by [`Bins`] like any other) with a
//! capacity, and it **falls by [`StoreRules::falls_per_meal`] percent of its
//! capacity for every meal anybody takes out of any fridge**
//! ([`Superfridge::served`], from `Effect::Buy`): a fridge stands for its
//! share of the superfridge. **Empty, no fridge in the world has food in it**
//! — only water ([`Superfridge::feeds_fridges`], which `Think` hands every
//! brain: `goals::choose_nearest` finds no food, `TakeItem` takes none). A map
//! with no superfridge on it keeps its fridges full, as it always has.
//!
//! A fall is a small share of a unit, so the superfridge keeps what has
//! fallen since the last whole unit ([`Superfridge::level`]): a unit comes out
//! of the store when a whole one has. Units come in a meal at a time, brought
//! from the warehouse by a courier the courier dispatch sent
//! ([`super::dispatch`]) when a shopkeeper found it at
//! [`StoreRules::call_at`] percent or less (`brain::goals::ShopGoal`).
//!
//! Plain Rust, a field of `Facilities`, read by every brain through
//! `Think::superfridge`, changed only by the world step's effects.

use std::sync::LazyLock;

use serde::Deserialize;

use crate::map::{Map, ObjectLayer, Point};
use crate::save::{invalid, pb, Invalid};

use super::bins::Bins;
use super::feature::{self, FeatureKind};
use super::item::ItemKind;
use super::log::Log;

/// What the store is run by, read from `store.yaml` ([`StoreRules::from_yaml`]);
/// the checked-in file is the defaults ([`StoreRules::default`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StoreRules {
    /// Normalized reserve units a superfridge holds; 1 is the default full stock.
    pub capacity: f64,
    /// Reserve units supplied by one warehouse delivery batch.
    pub delivery_size: f64,
    /// How much of its capacity it falls by for every meal taken out of any
    /// fridge, in percent.
    pub falls_per_meal: f64,
    /// At or below how full, in percent, a shopkeeper calls the courier
    /// dispatch.
    pub call_at: f64,
    /// How full a new game's superfridge is, in percent, where the map does
    /// not say ([`Map::initial_stock`]).
    pub starts_at: f64,
}

/// `store.yaml` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreFile {
    capacity: f64,
    #[serde(rename = "delivery size")]
    delivery_size: f64,
    #[serde(rename = "falls per meal")]
    falls_per_meal: f64,
    #[serde(rename = "call at")]
    call_at: f64,
    #[serde(rename = "starts at")]
    starts_at: f64,
}

/// The file the defaults are, built into the game.
pub const DEFAULTS_YAML: &str = include_str!("../../store.yaml");

static DEFAULTS: LazyLock<StoreRules> =
    LazyLock::new(|| StoreRules::from_yaml(DEFAULTS_YAML).unwrap_or_else(|error| panic!("store.yaml: {error}")));

/// The checked-in `store.yaml`: the one place a default is written.
impl Default for StoreRules {
    fn default() -> StoreRules {
        *DEFAULTS
    }
}

/// The most units a superfridge may be said to hold.
const MOST_CAPACITY: u32 = 1_000_000;

impl StoreRules {
    /// The rules `store.yaml` written as `text` says: every number named,
    /// nothing else, a capacity of a unit at least and every percentage
    /// between 0 and 100.
    pub fn from_yaml(text: &str) -> Result<StoreRules, String> {
        let file: StoreFile = serde_yaml::from_str(text).map_err(|error| error.to_string())?;
        let rules = StoreRules {
            capacity: file.capacity,
            delivery_size: file.delivery_size,
            falls_per_meal: file.falls_per_meal,
            call_at: file.call_at,
            starts_at: file.starts_at,
        };
        rules.check()?;
        Ok(rules)
    }

    /// Whether these are rules a store can be run by.
    fn check(&self) -> Result<(), String> {
        if !self.capacity.is_finite() || self.capacity <= 0.0 || self.capacity > MOST_CAPACITY as f64 {
            return Err(format!("capacity: {} is not positive and at most {MOST_CAPACITY}", self.capacity));
        }
        let batches = self.capacity / self.delivery_size;
        if !self.delivery_size.is_finite() || self.delivery_size <= 0.0
            || !batches.is_finite() || !(1.0..=MOST_CAPACITY as f64).contains(&batches)
            || (batches - batches.round()).abs() > 1e-8 {
            return Err("delivery size must divide capacity into 1..=1000000 whole batches".to_string());
        }
        for (name, value) in [("falls per meal", self.falls_per_meal), ("call at", self.call_at), ("starts at", self.starts_at)] {
            if !(value.is_finite() && (0.0..=100.0).contains(&value)) {
                return Err(format!("{name}: {value} is not between 0 and 100"));
            }
        }
        Ok(())
    }

    /// Warehouse delivery batches in a new game's reserve, rounded to the
    /// nearest batch. Defaults: 40 batches represent 0.4 of a capacity of 1.
    pub fn initial(&self) -> u32 {
        (self.batches() as f64 * self.starts_at / 100.0).round() as u32
    }

    /// Physical warehouse batches in a full reserve. Bins and inventories
    /// count these; percentages and public levels use normalized units.
    pub fn batches(&self) -> u32 {
        (self.capacity / self.delivery_size).round() as u32
    }

    pub(crate) fn save(&self) -> pb::StoreRules {
        let StoreRules { capacity, delivery_size, falls_per_meal, call_at, starts_at } = *self;
        pb::StoreRules { capacity: self.batches(), normalized_capacity: Some(capacity), delivery_size: Some(delivery_size), falls_per_meal, call_at, starts_at }
    }

    pub(crate) fn restore(saved: &pb::StoreRules) -> Result<StoreRules, Invalid> {
        let (capacity, delivery_size) = match (saved.normalized_capacity, saved.delivery_size) {
            (Some(capacity), Some(delivery_size)) => (capacity, delivery_size),
            // Old saves measured the same reserve in whole delivery batches.
            // Convert the units, preserving bins, consumption and prices.
            (None, None) => (saved.capacity as f64 / 100.0, 0.01),
            _ => return invalid("incomplete normalized store rules".to_string()),
        };
        let rules = StoreRules {
            capacity,
            delivery_size,
            falls_per_meal: saved.falls_per_meal,
            call_at: saved.call_at,
            starts_at: saved.starts_at,
        };
        match rules.check() {
            Ok(()) if rules.batches() == saved.capacity => Ok(rules),
            Ok(()) => invalid("store capacity disagrees with its delivery batch count".to_string()),
            Err(error) => invalid(format!("store rules: {error}")),
        }
    }
}

/// Every superfridge on the map, the rules they are run by, and what has
/// fallen since the last whole unit came out.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Superfridge {
    rules: StoreRules,
    /// Every superfridge's cell, sorted. Fixed by the map.
    cells: Vec<Point>,
    /// The share of a delivery batch fallen since the last whole one was taken out of
    /// the store, 0 to 1. Saved: it is the fraction [`Bins`] cannot hold.
    fallen: f64,
}

impl Superfridge {
    /// Every superfridge on `map`, run by the default rules, nothing fallen.
    pub fn from_map(map: &Map) -> Superfridge {
        let mut cells: Vec<Point> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|object| feature::store_of(object.kind.as_str()) == Some(FeatureKind::Superfridge))
            .map(|object| object.cell())
            .collect();
        cells.sort_unstable();
        cells.dedup();
        Superfridge { rules: StoreRules::default(), cells, fallen: 0.0 }
    }

    pub fn rules(&self) -> StoreRules {
        self.rules
    }

    /// Run by `rules` from now on — a new game's, from `store.yaml` — and
    /// stocked as they say ([`Superfridge::stock`]).
    pub fn set_rules(&mut self, rules: StoreRules, map: &Map, bins: &mut Bins) {
        self.rules = rules;
        self.stock(map, bins);
    }

    /// A new game's food: every superfridge the map does not stock itself
    /// holds [`StoreRules::initial`], nothing fallen.
    pub fn stock(&mut self, map: &Map, bins: &mut Bins) {
        self.fallen = 0.0;
        for &cell in &self.cells {
            if !map.initial_stock().iter().any(|stock| stock.cell == cell) {
                let _ = bins.set(cell, self.rules.initial());
            } else if bins.stored(cell) > self.rules.batches() {
                let _ = bins.set(cell, self.rules.batches());
            }
        }
    }

    /// Every superfridge, by cell.
    pub fn cells(&self) -> &[Point] {
        &self.cells
    }

    /// Whether the map has one: without, the fridges are never short of food.
    pub fn exists(&self) -> bool {
        !self.cells.is_empty()
    }

    /// How many units every superfridge together holds when full.
    pub fn capacity(&self) -> f64 {
        self.rules.capacity * self.cells.len() as f64
    }

    /// Food held in normalized reserve units: physical delivery batches,
    /// minus the partial debit, converted by the saved delivery size.
    pub fn level(&self, bins: &Bins) -> f64 {
        let stored: f64 = self.cells.iter().map(|&cell| bins.stored(cell) as f64).sum();
        (stored - self.fallen).max(0.0) * self.rules.delivery_size
    }

    /// How full they are, in percent of [`Superfridge::capacity`]; 0 with
    /// none on the map.
    pub fn percent(&self, bins: &Bins) -> f64 {
        match self.capacity() {
            0.0 => 0.0,
            capacity => self.level(bins) / capacity * 100.0,
        }
    }

    /// Fullness of one store. The shared fractional debit belongs to the
    /// fullest fridge, which will supply the next whole unit.
    pub fn percent_at(&self, cell: Point, bins: &Bins) -> f64 {
        if self.cells.binary_search(&cell).is_err() {
            return 0.0;
        }
        let debit = if self.fullest(bins) == Some(cell) { self.fallen } else { 0.0 };
        (bins.stored(cell) as f64 - debit).max(0.0) / self.rules.batches() as f64 * 100.0
    }

    pub fn wants_food_at(&self, cell: Point, bins: &Bins) -> bool {
        self.cells.binary_search(&cell).is_ok() && self.percent_at(cell, bins) <= self.rules.call_at
    }

    fn fullest(&self, bins: &Bins) -> Option<Point> {
        self.cells.iter().copied().max_by_key(|&cell| (bins.stored(cell), std::cmp::Reverse(cell)))
    }

    /// Whether a shopkeeper finding them this full calls the courier
    /// dispatch: at [`StoreRules::call_at`] or less.
    pub fn wants_food(&self, bins: &Bins) -> bool {
        self.exists() && self.percent(bins) <= self.rules.call_at
    }

    /// **Whether the fridges have food in them**: no superfridge on the map,
    /// or some food in one. Water is to be had either way.
    pub fn feeds_fridges(&self, bins: &Bins) -> bool {
        self.cells.is_empty() || self.cells.iter().any(|&cell| bins.stored(cell) > 0)
    }

    /// How many more units the superfridge in `cell` has room for.
    pub fn room(&self, cell: Point, bins: &Bins) -> u32 {
        self.rules.batches().saturating_sub(bins.stored(cell))
    }

    /// Somebody took `item` out of a fridge: a meal or a dog's dinner is
    /// [`StoreRules::falls_per_meal`] of the capacity off the superfridge, a
    /// whole unit out of the store each time a whole one has fallen — out of
    /// the fullest, the lower cell on a tie. The moment it is empty, the
    /// log says so. Water costs it nothing.
    pub(crate) fn served(&mut self, item: ItemKind, bins: &mut Bins, log: &Log) {
        if !matches!(item, ItemKind::Food | ItemKind::DogFood) || !self.feeds_fridges(bins) || self.cells.is_empty() {
            return;
        }
        self.fallen += self.rules.batches() as f64 * self.cells.len() as f64 * self.rules.falls_per_meal / 100.0;
        while self.fallen >= 1.0 {
            let fullest = self.fullest(bins);
            if !fullest.is_some_and(|cell| bins.take(cell)) {
                break;
            }
            self.fallen -= 1.0;
        }
        if !self.feeds_fridges(bins) {
            self.fallen = 0.0;
            log.important("the superfridge is empty: no fridge has food in it".to_string());
        }
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Store {
        let Superfridge { rules, cells: _, fallen } = self;
        pb::Store { rules: Some(rules.save()), fallen: *fallen }
    }

    /// The rules and what had fallen, onto the superfridges found on the
    /// same map. A save from before there were stores says nothing: the
    /// default rules, nothing fallen.
    pub(crate) fn restore(&mut self, saved: Option<&pb::Store>) -> Result<(), Invalid> {
        let Some(saved) = saved else {
            self.rules = StoreRules::default();
            self.fallen = 0.0;
            return Ok(());
        };
        self.rules = StoreRules::restore(pb::required(&saved.rules, "store rules")?)?;
        if !(saved.fallen.is_finite() && (0.0..1.0).contains(&saved.fallen)) {
            return invalid(format!("{} of a unit fallen from the superfridge", saved.fallen));
        }
        self.fallen = saved.fallen;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR};
    use crate::sim::testing::prop_at;

    const FRIDGE: Point = Point::new(3, 3);

    fn store(rules: StoreRules) -> (Superfridge, Bins, Map) {
        let mut map = Map::new(Size::new(8, 8), FLOOR);
        prop_at(&mut map, "superfridge", FRIDGE);
        prop_at(&mut map, "fridge", Point::new(6, 6));
        let mut bins = Bins::from_map(&map);
        let mut superfridge = Superfridge::from_map(&map);
        superfridge.set_rules(rules, &map, &mut bins);
        (superfridge, bins, map)
    }

    fn rules(capacity: u32, falls_per_meal: f64, starts_at: f64) -> StoreRules {
        StoreRules { capacity: capacity as f64, delivery_size: 1.0, falls_per_meal, call_at: 30.0, starts_at }
    }

    #[test]
    fn store_yaml_is_the_defaults_and_refuses_what_it_does_not_know() {
        let rules = StoreRules::from_yaml(DEFAULTS_YAML).unwrap();
        assert_eq!(rules, StoreRules::default());
        assert_eq!(
            (rules.capacity, rules.delivery_size, rules.falls_per_meal, rules.call_at, rules.starts_at),
            (1.0, 0.01, 0.014, 40.0, 40.0),
            "called at what it starts at, so it holds there"
        );
        let unknown = format!("{DEFAULTS_YAML}\nprice: 3\n");
        assert!(StoreRules::from_yaml(&unknown).is_err(), "a key nobody reads");
        let missing = DEFAULTS_YAML.replace("call at: 40", "");
        assert!(StoreRules::from_yaml(&missing).is_err());
        let over = DEFAULTS_YAML.replace("call at: 40", "call at: 140");
        assert!(StoreRules::from_yaml(&over).unwrap_err().contains("call at"));
        let empty = DEFAULTS_YAML.replace("capacity: 1", "capacity: 0");
        assert!(StoreRules::from_yaml(&empty).unwrap_err().contains("capacity"));
        for size in ["0", "-1", ".nan", "0.03", "2"] {
            let bad = DEFAULTS_YAML.replace("delivery size: 0.01", &format!("delivery size: {size}"));
            assert!(StoreRules::from_yaml(&bad).is_err(), "invalid delivery size {size}");
        }
    }

    #[test]
    fn normalized_capacity_one_starts_at_point_four_and_each_delivery_replaces_seventy_one_meals() {
        let (mut reserve, mut bins, _) = store(StoreRules::default());
        assert_eq!(reserve.capacity(), 1.0);
        assert_eq!(bins.stored(FRIDGE), 40, "warehouse batches");
        assert!((reserve.level(&bins) - 0.4).abs() < 1e-12);
        assert_eq!(reserve.percent(&bins), 40.0);
        assert!(reserve.wants_food_at(FRIDGE, &bins), "it starts at the call level");
        let before = reserve.level(&bins);
        let _ = bins.add(FRIDGE, 1);
        assert!((reserve.level(&bins) - before - 0.01).abs() < 1e-12);
        assert!(!reserve.wants_food_at(FRIDGE, &bins));
        let log = Log::new();
        for _ in 0..71 { reserve.served(ItemKind::Food, &mut bins, &log); }
        assert!(!reserve.wants_food_at(FRIDGE, &bins), "a delivery is seventy-one meals' worth");
        reserve.served(ItemKind::DogFood, &mut bins, &log);
        assert!(reserve.wants_food_at(FRIDGE, &bins));
    }

    #[test]
    fn old_whole_capacity_saves_convert_to_normalized_units_without_changing_stock() {
        let saved = pb::StoreRules { capacity: 100, falls_per_meal: 0.018, call_at: 30.0, starts_at: 40.0,
            normalized_capacity: None, delivery_size: None };
        let rules = StoreRules::restore(&saved).unwrap();
        assert_eq!(rules, StoreRules { capacity: 1.0, delivery_size: 0.01, falls_per_meal: 0.018, call_at: 30.0, starts_at: 40.0 });
        assert_eq!(rules.batches(), 100);
        assert_eq!(rules.save().capacity, 100, "older readers still see the right batch capacity");
    }

    #[test]
    fn a_new_superfridge_starts_as_full_as_the_rules_say_unless_the_map_stocks_it() {
        let (superfridge, bins, _) = store(rules(100, 0.018, 40.0));
        assert_eq!(bins.stored(FRIDGE), 40);
        assert_eq!((superfridge.level(&bins), superfridge.percent(&bins)), (40.0, 40.0));
        let mut map = Map::new(Size::new(8, 8), FLOOR);
        prop_at(&mut map, "superfridge", FRIDGE);
        assert!(map.set_initial_stock(FRIDGE, 7));
        let mut bins = Bins::from_map(&map);
        Superfridge::from_map(&map).set_rules(rules(100, 0.018, 40.0), &map, &mut bins);
        assert_eq!(bins.stored(FRIDGE), 7, "the map's own stock");
    }

    #[test]
    fn every_meal_out_of_a_fridge_takes_its_share_of_the_capacity_and_water_nothing() {
        let (mut superfridge, mut bins, _) = store(rules(100, 0.018, 40.0));
        let log = Log::new();
        for _ in 0..100 {
            superfridge.served(ItemKind::Water, &mut bins, &log);
        }
        assert_eq!(superfridge.level(&bins), 40.0, "water is free");
        // 0.018% of 100 units is 0.018 of a unit: 56 meals take a whole one.
        for _ in 0..55 {
            superfridge.served(ItemKind::Food, &mut bins, &log);
        }
        assert_eq!(bins.stored(FRIDGE), 40, "less than a unit has fallen");
        assert!((superfridge.level(&bins) - (40.0 - 55.0 * 0.018)).abs() < 1e-9);
        superfridge.served(ItemKind::DogFood, &mut bins, &log);
        assert_eq!(bins.stored(FRIDGE), 39, "a dog's dinner is a meal too");
        assert!((superfridge.percent(&bins) - (40.0 - 56.0 * 0.018)).abs() < 1e-9);
    }

    #[test]
    fn empty_it_feeds_no_fridge_until_food_comes_in_again() {
        let (mut superfridge, mut bins, _) = store(rules(10, 10.0, 20.0));
        let log = Log::new();
        assert_eq!(bins.stored(FRIDGE), 2);
        assert!(superfridge.feeds_fridges(&bins));
        superfridge.served(ItemKind::Food, &mut bins, &log);
        assert!(superfridge.feeds_fridges(&bins), "one unit left");
        assert!(superfridge.wants_food(&bins), "10% is under 30%");
        superfridge.served(ItemKind::Food, &mut bins, &log);
        assert!(!superfridge.feeds_fridges(&bins), "empty");
        assert_eq!(superfridge.level(&bins), 0.0);
        assert!(log.drain().iter().any(|line| line.contains("the superfridge is empty")));
        superfridge.served(ItemKind::Food, &mut bins, &log);
        assert_eq!(superfridge.level(&bins), 0.0, "nothing falls out of nothing");
        assert_eq!(bins.add(FRIDGE, 1), Some(1));
        assert!(superfridge.feeds_fridges(&bins));
        assert_eq!(superfridge.room(FRIDGE, &bins), 9);
    }

    #[test]
    fn a_map_with_no_superfridge_always_feeds_its_fridges() {
        let map = Map::new(Size::new(8, 8), FLOOR);
        let (mut superfridge, mut bins) = (Superfridge::from_map(&map), Bins::from_map(&map));
        superfridge.served(ItemKind::Food, &mut bins, &Log::new());
        assert!(superfridge.feeds_fridges(&bins));
        assert!(!superfridge.wants_food(&bins));
    }

    #[test]
    fn each_shop_calls_for_its_own_stock_even_when_another_shop_is_full() {
        let (_, _, mut map) = store(rules(100, 0.018, 100.0));
        let empty = Point::new(2, 2);
        prop_at(&mut map, "superfridge", empty);
        let mut bins = Bins::from_map(&map);
        let mut fridge = Superfridge::from_map(&map);
        fridge.set_rules(rules(100, 0.018, 100.0), &map, &mut bins);
        assert!(bins.set(empty, 0));
        assert_eq!(fridge.percent(&bins), 50.0);
        assert!(fridge.wants_food_at(empty, &bins));
        assert!(!fridge.wants_food_at(FRIDGE, &bins));
        assert!(!fridge.wants_food_at(Point::new(0, 0), &bins));
    }

    #[test]
    fn the_rules_and_what_has_fallen_survive_a_save() {
        let (mut superfridge, mut bins, map) = store(rules(50, 0.5, 60.0));
        superfridge.served(ItemKind::Food, &mut bins, &Log::new());
        let saved = superfridge.save();
        let mut restored = Superfridge::from_map(&map);
        restored.restore(Some(&saved)).unwrap();
        assert_eq!(restored, superfridge);
        let mut broken = saved;
        broken.fallen = 1.5;
        assert!(restored.restore(Some(&broken)).is_err());
        restored.restore(None).unwrap();
        assert_eq!(restored.rules(), StoreRules::default());
    }
}
