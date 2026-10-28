//! The **store**: on the lot nearest the warehouse that is left once the
//! factories have theirs, so a courier the dispatch sends for its food walks
//! little (`sim::dispatch`). Its [`SUPERFRIDGE`] is the district's food —
//! every fridge's share of it (`sim::superfridge`) — and the store courier's
//! workplace; its [`SHOP_COUNTER`] is the four shopkeepers' (`sim::job`).

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, LOT_H, LOT_W};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "store";
/// The district's food, in the middle of the far wall: one, and its one
/// store courier.
pub const SUPERFRIDGE: &str = "superfridge";
pub const SUPERFRIDGES: usize = 1;
/// Where the shopkeepers mind the store, against a side wall by the door:
/// one, kept by four, a quarter of the day each.
pub const SHOP_COUNTER: &str = "shop counter";
pub const COUNTERS: usize = 1;
const WALL: &str = "wall brown";
const FLOOR: &str = "floor red";

/// One, on the lot whose middle is nearest the warehouse's — of those the
/// factories left. Draws nothing.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    whole_block: false,
    wanted: |_| true,
    rank: |siting, _, lot| {
        let warehouse = siting.the(super::warehouse::NAME);
        let to = Point::new(warehouse.x + warehouse.w / 2, warehouse.y + warehouse.h / 2);
        lot.origin.offset(LOT_W / 2, LOT_H / 2).manhattan_distance(to)
    },
    build: |_, map, props, lot| store(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[SUPERFRIDGE, SHOP_COUNTER],
};

/// The store, on `lot`: the whole lot, `WALL` round `FLOOR`, a plain door —
/// nobody's property — in the middle of the lot's first street side. Laid out
/// from the door ([`Room`]): the [`SHOP_COUNTER`] against a side wall in the
/// row past the one inside the door, and the [`SUPERFRIDGE`] in the middle of
/// the far wall, a few steps apart, with open floor round both. Draws nothing
/// from the RNG.
fn store(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
    let walls = Rect::new(lot.origin.x, lot.origin.y, LOT_W, LOT_H);
    build(map, walls, WALL, FLOOR);
    let side = lot.outer[0];
    let door: Vec<Point> = match side {
        Side::South | Side::North => {
            let y = if side == Side::South { walls.y } else { walls.top() };
            (0..DOOR_WIDTH).map(|i| Point::new(walls.x + walls.w / 2 - 1 + i, y)).collect()
        }
        Side::West | Side::East => {
            let x = if side == Side::West { walls.x } else { walls.right() };
            (0..DOOR_WIDTH).map(|i| Point::new(x, walls.y + walls.h / 2 - 1 + i)).collect()
        }
    };
    for cell in door {
        map.set_terrain(cell, tile(FLOOR));
        props.push(object(DOOR, cell));
    }
    let inside = walls.interior();
    let (wide, deep) = match side {
        Side::South | Side::North => (inside.w, inside.h),
        Side::West | Side::East => (inside.h, inside.w),
    };
    let room = Room { walls, side, wide, deep };
    props.push(object(SHOP_COUNTER, room.at(0, 1)));
    props.push(object(SUPERFRIDGE, room.at(wide / 2, deep - 1)));
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One store: its superfridge and its counter and nothing else, nobody's,
    /// each with a free cell beside it, on the lot nearest the warehouse the
    /// factories left, and neither anywhere else.
    #[test]
    fn a_district_has_one_store_near_its_warehouse() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let store = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| store.interior().contains(prop.cell()));
            assert!(
                inside.clone().all(|prop| [SUPERFRIDGE, SHOP_COUNTER].contains(&prop.kind.as_str())),
                "seed {seed}: the store holds its superfridge and counter only"
            );
            for (name, count) in [(SUPERFRIDGE, SUPERFRIDGES), (SHOP_COUNTER, COUNTERS)] {
                assert_eq!(props_in(map, store, name), count, "seed {seed}: {name}");
                assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), name), count, "seed {seed}: {name} elsewhere");
            }
            for prop in inside {
                let free = prop.cell().cardinal_neighbours().into_iter().any(|side| store.interior().contains(side) && map.is_passable(side));
                assert!(free, "seed {seed}: nowhere to stand at the {} at {:?}", prop.kind.as_str(), prop.cell());
            }
            let properties = crate::sim::Properties::from_map(map);
            for cell in store.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the store is nobody's property");
            }
            let warehouse = district.the(warehouse::NAME);
            let middle = |rect: Rect| Point::new(rect.x + rect.w / 2, rect.y + rect.h / 2);
            let to_the_store = middle(store).manhattan_distance(middle(warehouse));
            // Compare lots that were available to the builder. The spare
            // park in the old shops' block was never a candidate, and a
            // house can be offset within its lot.
            for lot in (0..BLOCKS_Y).flat_map(|by| (0..BLOCKS_X).flat_map(move |bx| lots_of(bx, by))) {
                let other = Rect::new(lot.origin.x, lot.origin.y, LOT_W, LOT_H);
                let available = district.houses.iter().any(|house| other.contains(Point::new(house.walls.x, house.walls.y)))
                    || (district.parks.contains(&other) && !district.shops.iter().any(|shop| lot.block().contains(Point::new(shop.x, shop.y))));
                if !available { continue; }
                assert!(
                    middle(other).manhattan_distance(middle(warehouse)) >= to_the_store,
                    "seed {seed}: the lot at {other:?} is nearer the warehouse than the store at {store:?}"
                );
            }
        }
    }
}
