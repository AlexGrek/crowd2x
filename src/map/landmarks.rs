//! The buildings every district has, found again from a map's props.
//!
//! The generator knows where each lot building went ([`District::lot_buildings`]),
//! but a map file and a saved game keep only tiles and props — so the find menu
//! cannot ask the district. It looks for what each building cannot be without
//! instead: the warehouse is wherever the scrap bins and racks are, the
//! hospital wherever the beds are, a restaurant wherever a toilet stands on the
//! restaurants' yellow tiles (a house's and the bank's stand on other floors).
//! Props of one kind that lie close together are one building, so the two
//! crematoria are the one crematorium and the four restaurants are four.
//!
//! A landmark is a *place to look at*, not a rectangle: its [`Landmark::at`]
//! is the middle of the props that name it. The bank has no prop of its own
//! (its computers and fridges are every house's), so it is not listed.
//!
//! [`District::lot_buildings`]: super::district::District

use std::collections::BTreeMap;

use super::district::{crematorium, farm, food_factory, hospital, palace, playzone, power_plant, restaurant, steel_factory, store, warehouse};
use super::{Map, ObjectLayer, Point, TerrainId};

/// How far apart, in cells either way, two props of a building may be and
/// still be in the same one. A lot is 8 cells by 7, and two buildings of a kind
/// are a street and more apart.
const NEIGHBOURS: i32 = 9;

/// A kind of building, and how to recognise one.
pub struct LandmarkDef {
    /// What the find menu calls it.
    pub name: &'static str,
    /// The props that only it has (any of them will do).
    props: &'static [&'static str],
    /// Only those standing on this tile: how a restaurant's toilet is told
    /// from a house's.
    floor: Option<&'static str>,
}

/// How many kinds of building there are.
pub const KINDS: usize = 11;

/// Every building a district must have, in the order the find menu lists them.
pub static LANDMARKS: [LandmarkDef; KINDS] = [
    LandmarkDef {
        name: warehouse::NAME,
        props: &[warehouse::SCRAP_BIN, warehouse::STEEL_RACK, warehouse::MEAL_RACK, warehouse::FISH_BIN],
        floor: None,
    },
    LandmarkDef { name: restaurant::NAME, props: &["toilet"], floor: Some(restaurant::FLOOR) },
    LandmarkDef { name: crematorium::NAME, props: &[crematorium::CREMATORIUM], floor: None },
    LandmarkDef {
        name: steel_factory::NAME,
        props: &[steel_factory::FURNACE, steel_factory::SCRAP_HOPPER, steel_factory::STEEL_PALLET],
        floor: None,
    },
    LandmarkDef { name: farm::NAME, props: &[farm::GRAIN_BIN], floor: None },
    LandmarkDef { name: power_plant::NAME, props: &[power_plant::GENERATOR, power_plant::CONTROL_PANEL], floor: None },
    LandmarkDef {
        name: food_factory::NAME,
        props: &[food_factory::COOKER, food_factory::WHEAT_HOPPER, food_factory::MEAL_PALLET],
        floor: None,
    },
    LandmarkDef { name: playzone::NAME, props: &[playzone::PLAYZONE_COMPUTER, playzone::CABINET], floor: None },
    LandmarkDef { name: hospital::NAME, props: &["hospital bed", "nurse station"], floor: None },
    LandmarkDef { name: palace::NAME, props: &[palace::THRONE], floor: None },
    LandmarkDef { name: store::NAME, props: &[store::SUPERFRIDGE, store::SHOP_COUNTER], floor: None },
];

/// One building found on a map.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Landmark {
    /// Its row in [`LANDMARKS`].
    pub def: usize,
    /// The middle of its props.
    pub at: Point,
}

/// Every landmark on `map`, kind by kind in [`LANDMARKS`] order and the same
/// kind in the order of the cells they are at. A map without any gives none.
pub fn find(map: &Map) -> Vec<Landmark> {
    let mut found = Vec::new();
    for (def, landmark) in LANDMARKS.iter().enumerate() {
        let floor = landmark.floor.map(TerrainId::from_name);
        let on_floor = |cell: Point| match floor {
            None => true,
            Some(None) => false,
            Some(Some(id)) => map.terrain(cell) == Some(id),
        };
        let mut cells: Vec<Point> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|prop| landmark.props.contains(&prop.kind.as_str()))
            .map(|prop| prop.cell())
            .filter(|&cell| on_floor(cell))
            .collect();
        cells.sort_unstable();
        cells.dedup();
        found.extend(middles(&cells).into_iter().map(|at| Landmark { def, at }));
    }
    found
}

/// The middle of each group of `cells` that lie within [`NEIGHBOURS`] of one
/// another, group by group.
fn middles(cells: &[Point]) -> Vec<Point> {
    let mut group: Vec<usize> = (0..cells.len()).collect();
    fn root(group: &mut [usize], mut at: usize) -> usize {
        while group[at] != at {
            group[at] = group[group[at]];
            at = group[at];
        }
        at
    }
    for i in 0..cells.len() {
        for j in 0..i {
            let (a, b) = (cells[i], cells[j]);
            if (a.x - b.x).abs().max((a.y - b.y).abs()) <= NEIGHBOURS {
                let (a, b) = (root(&mut group, i), root(&mut group, j));
                group[a] = b;
            }
        }
    }
    let mut corners: BTreeMap<usize, (Point, Point)> = BTreeMap::new();
    for (i, &cell) in cells.iter().enumerate() {
        let root = root(&mut group, i);
        let (low, high) = corners.entry(root).or_insert((cell, cell));
        *low = Point::new(low.x.min(cell.x), low.y.min(cell.y));
        *high = Point::new(high.x.max(cell.x), high.y.max(cell.y));
    }
    let mut middles: Vec<Point> = corners
        .into_values()
        .map(|(low, high)| Point::new((low.x + high.x) / 2, (low.y + high.y) / 2))
        .collect();
    middles.sort_unstable();
    middles
}
