//! The **warehouse**: where the scrap collectors store what they bring in
//! from the wasteland, and the couriers the steel the factory makes.
//!
//! On the lot facing the street round the edge of the district that is
//! nearest a heap of the district's own wasteland
//! ([`heaps_round`](super::heaps_round)), so its collectors' trips are
//! short; a [`SCRAP_BIN`] in each corner and a [`STEEL_RACK`] on the long
//! wall away from the door. Picking the lot nearest the scrap rather than the
//! first edge lot took the walk from the warehouse to the nearest heap from a
//! median of 55 cells (max 137) to 21-33 for 63 of 64 seeds.

use rand::rngs::SmallRng;
use rand::RngExt;

use super::{build, door_clear_of_corners, object, place_in, tile, Lot, LotBuilding, Rect, Side, DOOR, DOOR_WIDTH, LOT_H, LOT_W};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "warehouse";
/// Where a scrap collector stores what it brings in: one in each corner of
/// the warehouse, and one collector to a bin (`sim::job`).
pub const SCRAP_BIN: &str = "scrap bin";
/// How many bins the warehouse has, and so how many collectors a district
/// can employ.
pub const WAREHOUSE_BINS: usize = 4;
/// Where the steel a courier brings from the steel factory is kept: one on
/// the warehouse's long wall away from its door (`sim::bins`).
pub const STEEL_RACK: &str = "steel rack";
pub const MEAL_RACK: &str = "meal rack";
pub const INITIAL_MEALS: u32 = 30;
const WALL: &str = "wall brown";
const FLOOR: &str = "floor diagonal";

/// One, on the edge lot nearest the scrap: ties to the first in the shuffled
/// order. The wasteland is the seed's as much as the district is, so this
/// draws nothing and is the same every time.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    wanted: |lot| lot.edge_sides().next().is_some(),
    rank: |siting, _, lot| {
        let middle = lot.origin.offset(LOT_W / 2, LOT_H / 2);
        siting.heaps.iter().map(|&heap| heap.manhattan_distance(middle)).min().unwrap_or(i32::MAX)
    },
    build: warehouse,
    tiles: &[WALL, FLOOR],
    props: &[SCRAP_BIN, STEEL_RACK, MEAL_RACK],
};

/// The warehouse, on `lot`: a room as big as a shop, a plain door — anybody
/// may come in, it is nobody's property — in a wall facing the edge street,
/// and a [`SCRAP_BIN`] in each of its [`WAREHOUSE_BINS`] corners. The door is
/// kept two cells off the corners, so no bin stands in front of it, and each
/// bin has the two cells beside it free to be used from.
fn warehouse(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
    let walls = place_in(rng, lot, LOT_W - 1, LOT_H - 1);
    build(map, walls, WALL, FLOOR);
    let sides: Vec<Side> = lot.edge_sides().collect();
    let side = sides[rng.random_range(0..sides.len())];
    for cell in door_clear_of_corners(rng, walls, side, DOOR_WIDTH, 2) {
        map.set_terrain(cell, tile(FLOOR));
        props.push(object(DOOR, cell));
    }
    let inside = walls.interior();
    let corners = [
        Point::new(inside.x, inside.y),
        Point::new(inside.right(), inside.y),
        Point::new(inside.x, inside.top()),
        Point::new(inside.right(), inside.top()),
    ];
    for corner in corners {
        props.push(object(SCRAP_BIN, corner));
    }
    // The steel rack in the middle of the long wall away from the door: the
    // interior is five cells by four, so the middle of a long wall has a
    // cell either side of it before the corners' bins.
    let rack_y = if side == Side::North { inside.y } else { inside.top() };
    props.push(object(STEEL_RACK, Point::new(inside.x + inside.w / 2, rack_y)));
    // A second rack in the middle aisle, with space on every side and
    // neither corner bins nor either leaf of the entrance obstructed.
    let meals = Point::new(inside.x + inside.w / 2, inside.y + 1);
    props.push(object(MEAL_RACK, meals));
    assert!(map.set_initial_stock(meals, INITIAL_MEALS));
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{doors_of, props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One warehouse, its bins and its steel rack and nothing else in it,
    /// open to anybody, on the lot facing the street round the edge that is
    /// nearest the scrap.
    #[test]
    fn a_district_has_one_warehouse_of_bins_and_a_rack_on_the_edge_lot_nearest_the_scrap() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let warehouse = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| warehouse.interior().contains(prop.cell()));
            assert!(
                inside.clone().all(|prop| [SCRAP_BIN, STEEL_RACK, MEAL_RACK].contains(&prop.kind.as_str())),
                "seed {seed}: the warehouse holds bins and a rack only"
            );
            assert_eq!(inside.count(), WAREHOUSE_BINS + 2, "seed {seed}");
            assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), SCRAP_BIN), WAREHOUSE_BINS, "seed {seed}: bins nowhere else");
            assert_eq!(props_in(map, warehouse, STEEL_RACK), 1, "seed {seed}: a rack for the steel");
            assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), STEEL_RACK), 1, "seed {seed}: and none elsewhere");
            let properties = crate::sim::Properties::from_map(map);
            for cell in warehouse.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the warehouse is nobody's property");
            }
            let near_the_edge = warehouse.x - STREET <= 1
                || WIDTH - STREET - 1 - warehouse.right() <= 1
                || warehouse.y - STREET <= 1
                || HEIGHT - STREET - 1 - warehouse.top() <= 1;
            assert!(near_the_edge, "seed {seed}: the warehouse at {warehouse:?} is not on the edge");

            // Of the lots on the edge to be had — not the bank's or the
            // shops' blocks — the one nearest the scrap.
            let heaps = heaps_round(seed);
            let to_scrap = |lot: &Lot| {
                let middle = lot.origin.offset(LOT_W / 2, LOT_H / 2);
                heaps.iter().map(|&heap| heap.manhattan_distance(middle)).min().unwrap()
            };
            let taken = |bx: i32, by: i32| {
                let origin = block_origin(bx, by);
                let block = Rect::new(origin.x, origin.y, BLOCK_W, BLOCK_H);
                let bank = district.bank;
                [Point::new(bank.x, bank.y), Point::new(bank.right(), bank.top()), Point::new(district.shops[0].x, district.shops[0].y), Point::new(district.palace.x, district.palace.y)]
                    .into_iter()
                    .any(|cell| block.contains(cell))
            };
            let edge_lots: Vec<Lot> = (0..BLOCKS_Y)
                .flat_map(|by| (0..BLOCKS_X).map(move |bx| (bx, by)))
                .filter(|&(bx, by)| !taken(bx, by))
                .flat_map(|(bx, by)| lots_of(bx, by))
                .filter(|lot| lot.edge_sides().next().is_some())
                .collect();
            let ours = edge_lots
                .iter()
                .find(|lot| Rect::new(lot.origin.x, lot.origin.y, LOT_W, LOT_H).contains(Point::new(warehouse.x, warehouse.y)))
                .expect("the warehouse is on an edge lot");
            let nearest = edge_lots.iter().map(to_scrap).min().unwrap();
            assert_eq!(to_scrap(ours), nearest, "seed {seed}: the warehouse is not on the edge lot nearest the scrap");
        }
    }

    /// The warehouse's door faces the edge street, so a collector steps out
    /// of it towards the wasteland.
    #[test]
    fn the_warehouse_door_opens_onto_the_street_round_the_edge() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let warehouse = district.the(NAME);
            let doors: Vec<Point> = doors_of(map).into_iter().filter(|&(cell, _)| warehouse.contains(cell)).map(|(cell, _)| cell).collect();
            assert_eq!(doors.len(), DOOR_WIDTH as usize, "seed {seed}");
            for door in doors {
                let outside = door.cardinal_neighbours().into_iter().find(|&cell| !warehouse.contains(cell)).unwrap();
                let edge_street = outside.x < STREET || outside.x >= WIDTH - STREET || outside.y < STREET || outside.y >= HEIGHT - STREET;
                let yard = !edge_street && map.terrain(outside) == Some(tile(PAVEMENT));
                // A building is placed somewhere in its lot, so the door may
                // open onto the cell of yard between the lot's edge and it.
                assert!(edge_street || yard, "seed {seed}: the warehouse door at {door:?} opens onto {outside:?}");
                let toward_edge = |cell: Point| cell.x.min(cell.y).min(WIDTH - 1 - cell.x).min(HEIGHT - 1 - cell.y);
                assert!(toward_edge(outside) < toward_edge(door), "seed {seed}: the door at {door:?} faces inwards");
            }
        }
    }
}
