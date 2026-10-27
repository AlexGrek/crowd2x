//! The **warehouse**: where the scrap collectors store what they bring in
//! from the wasteland, and the couriers the steel the factory makes.
//!
//! On the block facing the street round the edge of the district that is
//! nearest a heap of the district's own wasteland
//! ([`heaps_round`](super::heaps_round)), so its collectors' trips are
//! short. It occupies nearly a whole block, with a [`SCRAP_BIN`] in each
//! corner, stores along the side walls and a clear aisle between two exits.

use rand::rngs::SmallRng;
use rand::RngExt;

use super::{build, object, tile, Lot, LotBuilding, Rect, Side, DOOR, DOOR_WIDTH};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "warehouse";
/// Where a scrap collector stores what it brings in: one in each corner of
/// the warehouse, and one collector to a bin (`sim::job`).
pub const SCRAP_BIN: &str = "scrap bin";
/// How many bins the warehouse has, and so how many collectors a district
/// can employ.
pub const WAREHOUSE_BINS: usize = 4;
/// Where steel from the factory is kept: along a warehouse side wall,
/// clear of both exits (`sim::bins`).
pub const STEEL_RACK: &str = "steel rack";
pub const MEAL_RACK: &str = "meal rack";
pub const INITIAL_MEALS: u32 = 30;
pub const FISH_BIN: &str = "fish bin";
pub const FISHERMEN: usize = 3;
const WALL: &str = "wall brown";
const FLOOR: &str = "floor diagonal";

/// One, on the edge block nearest the scrap: ties to the first in the shuffled
/// order. The wasteland is the seed's as much as the district is, so this
/// draws nothing and is the same every time.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    whole_block: true,
    wanted: |lot| lot.edge_sides().next().is_some(),
    rank: |siting, _, lot| {
        let block = lot.block();
        let middle = Point::new(block.x + block.w / 2, block.y + block.h / 2);
        siting.heaps.iter().map(|&heap| heap.manhattan_distance(middle)).min().unwrap_or(i32::MAX)
    },
    build: warehouse,
    tiles: &[WALL, FLOOR],
    props: &[SCRAP_BIN, STEEL_RACK, MEAL_RACK, FISH_BIN],
};

/// Nearly a whole block, with two opposite double-width exits joined by a
/// clear aisle. One exit faces the edge street; anybody may use either.
/// Corner scrap bins and stores along the side walls leave the aisle open.
fn warehouse(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
    let block = lot.block();
    let walls = Rect::new(block.x + rng.random_range(0..=1), block.y + rng.random_range(0..=1), block.w - 1, block.h - 1);
    build(map, walls, WALL, FLOOR);
    let sides: Vec<Side> = lot.edge_sides().collect();
    let side = sides[rng.random_range(0..sides.len())];
    let vertical = matches!(side, Side::South | Side::North);
    let along = if vertical { walls.w } else { walls.h };
    let start = along / 2 - 1;
    let aisle = |cell: Point| {
        let offset = if vertical { cell.x - walls.x } else { cell.y - walls.y };
        (start..start + DOOR_WIDTH).contains(&offset)
    };
    for cell in walls.cells().filter(|&cell| walls.on_edge(cell) && aisle(cell)) {
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
    let steel = if vertical {
        Point::new(inside.x, inside.y + inside.h / 2)
    } else {
        Point::new(inside.x + inside.w / 2, inside.y)
    };
    props.push(object(STEEL_RACK, steel));
    let meals = if vertical {
        Point::new(inside.right(), inside.y + inside.h / 2)
    } else {
        Point::new(inside.x + inside.w / 2, inside.top())
    };
    props.push(object(MEAL_RACK, meals));
    assert!(map.set_initial_stock(meals, INITIAL_MEALS));
    // Keep the doorways, every aisle and every store accessible as fish
    // storage is fitted around the existing warehouse furniture.
    let occupied = |cell: Point, props: &[Object]| props.iter().any(|p| p.cell() == cell && p.kind.as_str() != DOOR);
    let mut fitted = 0;
    for cell in inside.cells() {
        if occupied(cell, props) || aisle(cell) || cell.cardinal_neighbours().into_iter().any(|next| occupied(next, props)) {
            continue;
        }
        props.push(object(FISH_BIN, cell));
        let free: Vec<Point> = walls.cells().filter(|&p| map.terrain(p).is_some_and(|t| t.is_passable()) && !occupied(p, props)).collect();
        let mut reached = vec![free[0]];
        let mut cursor = 0;
        while cursor < reached.len() {
            for next in reached[cursor].cardinal_neighbours() {
                if free.contains(&next) && !reached.contains(&next) {
                    reached.push(next);
                }
            }
            cursor += 1;
        }
        let accessible = reached.len() == free.len() && props.iter().filter(|p| inside.contains(p.cell())).all(|p| {
            p.cell().cardinal_neighbours().iter().any(|side| reached.contains(side))
        });
        if accessible {
            fitted += 1;
            if fitted == FISHERMEN { break; }
        } else {
            props.pop();
        }
    }
    assert_eq!(fitted, FISHERMEN, "the warehouse needs three accessible fish bins");
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{doors_of, props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One warehouse, its bins and its steel rack and nothing else in it,
    /// open to anybody, on the block facing the street round the edge that is
    /// nearest the scrap, with over seven times the old interior area.
    #[test]
    fn a_district_has_one_large_warehouse_of_bins_and_racks_on_the_edge_block_nearest_the_scrap() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let warehouse = district.the(NAME);
            assert_eq!((warehouse.w, warehouse.h), (2 * LOT_W - 1, 2 * LOT_H - 1), "seed {seed}");
            assert!(warehouse.interior().w * warehouse.interior().h >= 7 * (LOT_W - 3) * (LOT_H - 3));
            for other in district.buildings().into_iter().filter(|&walls| walls != warehouse) {
                assert!(!other.cells().any(|cell| warehouse.contains(cell)), "seed {seed}: {other:?} overlaps the warehouse");
            }
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| warehouse.interior().contains(prop.cell()));
            assert!(
                inside.clone().all(|prop| [SCRAP_BIN, STEEL_RACK, MEAL_RACK, FISH_BIN].contains(&prop.kind.as_str())),
                "seed {seed}: the warehouse holds bins and a rack only"
            );
            for store in inside.clone() {
                let approaches = store.cell().cardinal_neighbours().into_iter().filter(|&cell| map.is_passable(cell)).count();
                assert!(approaches >= 2, "seed {seed}: storage at {:?} needs two clear approaches", store.cell());
            }
            assert_eq!(inside.count(), WAREHOUSE_BINS + 2 + FISHERMEN, "seed {seed}");
            assert_eq!(props_in(map, warehouse, FISH_BIN), FISHERMEN);
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

            // Of the blocks on the edge to be had — not the bank's or the
            // shops' blocks — the one nearest the scrap.
            let heaps = heaps_round(seed);
            let to_scrap = |lot: &Lot| {
                let block = lot.block();
                let middle = Point::new(block.x + block.w / 2, block.y + block.h / 2);
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
                .find(|lot| lot.block().contains(Point::new(warehouse.x, warehouse.y)))
                .expect("the warehouse is on an edge block");
            let nearest = edge_lots.iter().map(to_scrap).min().unwrap();
            assert_eq!(to_scrap(ours), nearest, "seed {seed}: the warehouse is not on the edge block nearest the scrap");
        }
    }

    /// Two opposite exits, one facing the edge street, with both leaves'
    /// aisles clear all the way through and out onto the pavement.
    #[test]
    fn the_warehouse_has_two_opposite_exits_and_a_clear_aisle_to_the_street() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let warehouse = district.the(NAME);
            let doors: Vec<Point> = doors_of(map).into_iter().filter(|&(cell, _)| warehouse.contains(cell)).map(|(cell, _)| cell).collect();
            assert_eq!(doors.len(), 2 * DOOR_WIDTH as usize, "seed {seed}: two double-width exits");
            let mut edge_leaves = 0;
            for &door in &doors {
                let outside = door.cardinal_neighbours().into_iter().find(|&cell| !warehouse.contains(cell)).unwrap();
                let edge_street = outside.x < STREET || outside.x >= WIDTH - STREET || outside.y < STREET || outside.y >= HEIGHT - STREET;
                let yard = !edge_street && map.terrain(outside) == Some(tile(PAVEMENT));
                // A building is placed somewhere in its lot, so the door may
                // open onto the cell of yard between the lot's edge and it.
                assert!(edge_street || yard, "seed {seed}: the warehouse door at {door:?} opens onto {outside:?}");
                let toward_edge = |cell: Point| cell.x.min(cell.y).min(WIDTH - 1 - cell.x).min(HEIGHT - 1 - cell.y);
                if toward_edge(outside) < toward_edge(door) {
                    edge_leaves += 1;
                }
                let vertical = door.y == warehouse.y || door.y == warehouse.top();
                let opposite = if vertical {
                    Point::new(door.x, if door.y == warehouse.y { warehouse.top() } else { warehouse.y })
                } else {
                    Point::new(if door.x == warehouse.x { warehouse.right() } else { warehouse.x }, door.y)
                };
                assert!(doors.contains(&opposite), "seed {seed}: the door at {door:?} has no opposite exit");
                let aisle: Vec<Point> = if vertical {
                    (warehouse.y - 1..=warehouse.top() + 1).map(|y| Point::new(door.x, y)).collect()
                } else {
                    (warehouse.x - 1..=warehouse.right() + 1).map(|x| Point::new(x, door.y)).collect()
                };
                for cell in aisle {
                    assert!(map.is_passable(cell), "seed {seed}: an exit aisle is blocked at {cell:?}");
                }
            }
            assert_eq!(edge_leaves, DOOR_WIDTH as usize, "seed {seed}: one exit faces the edge street");
        }
    }
}
