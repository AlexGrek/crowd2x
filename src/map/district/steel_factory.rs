//! The **steel factory**: on the lot nearest the warehouse, so the couriers'
//! trips between the two are short. Its [`FURNACES`] are the steelworkers'
//! workplaces and its [`HOPPERS`] the couriers'; the [`PALLETS`] keep the
//! steel until a courier takes it to the warehouse's rack (`sim::job`).

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, LOT_H, LOT_W};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "steel factory";
/// A steelworker's workplace, where scrap metal is smelted into steel: one in
/// each of the far corners, so [`FURNACES`] steelworkers.
pub const FURNACE: &str = "furnace";
pub const FURNACES: usize = 2;
/// The factory's store of scrap metal to smelt, one against each side wall
/// by the door — a courier's workplace, so [`HOPPERS`] couriers.
pub const SCRAP_HOPPER: &str = "scrap hopper";
pub const HOPPERS: usize = 2;
/// The factory's store of the steel it made, one against each side wall
/// between the hoppers and the furnaces.
pub const STEEL_PALLET: &str = "steel pallet";
pub const PALLETS: usize = 2;
const WALL: &str = "block";
const FLOOR: &str = "floor";

/// One, on the lot whose middle is nearest the warehouse's. Draws nothing.
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
    build: |_, map, props, lot| steel_factory(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[FURNACE, SCRAP_HOPPER, STEEL_PALLET],
};

/// The steel factory, on `lot`: the whole lot, `WALL` round `FLOOR`, a plain
/// door — nobody's property — in the middle of the lot's first street side.
/// Laid out from the door ([`Room`]): a [`SCRAP_HOPPER`] against each side
/// wall in the row inside the door, a [`STEEL_PALLET`] against each side wall
/// two rows short of the far wall, and a [`FURNACE`] in each far corner;
/// everything has two free cells beside it, and the middle is open floor.
/// Draws nothing from the RNG.
fn steel_factory(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
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
    for across in [0, wide - 1] {
        props.push(object(SCRAP_HOPPER, room.at(across, 0)));
        props.push(object(STEEL_PALLET, room.at(across, deep - 3)));
        props.push(object(FURNACE, room.at(across, deep - 1)));
    }
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One steel factory: its furnaces, hoppers and pallets and nothing
    /// else, nobody's, on the lot nearest the warehouse, and none of them
    /// anywhere else.
    #[test]
    fn a_district_has_one_steel_factory_beside_its_warehouse() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let factory = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| factory.interior().contains(prop.cell()));
            assert!(
                inside.clone().all(|prop| [FURNACE, SCRAP_HOPPER, STEEL_PALLET].contains(&prop.kind.as_str())),
                "seed {seed}: the steel factory holds its furnaces and stores only"
            );
            for (name, count) in [(FURNACE, FURNACES), (SCRAP_HOPPER, HOPPERS), (STEEL_PALLET, PALLETS)] {
                assert_eq!(props_in(map, factory, name), count, "seed {seed}: {name}");
                assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), name), count, "seed {seed}: {name} elsewhere");
            }
            let properties = crate::sim::Properties::from_map(map);
            for cell in factory.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the steel factory is nobody's property");
            }
            let warehouse = district.the(warehouse::NAME);
            let middle = |rect: Rect| Point::new(rect.x + rect.w / 2, rect.y + rect.h / 2);
            let to_the_factory = middle(factory).manhattan_distance(middle(warehouse));
            let lot_middles = district.houses.iter().map(|house| house.walls).chain(district.parks.iter().copied());
            for other in lot_middles {
                assert!(
                    middle(other).manhattan_distance(middle(warehouse)) + 2 >= to_the_factory,
                    "seed {seed}: the lot at {other:?} is nearer the warehouse than the factory at {factory:?}"
                );
            }
        }
    }
}
