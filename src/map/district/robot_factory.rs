//! The **robot factory**: on the lot nearest the steel factory, so the
//! engineers' steel has a short way from the warehouse's rack. Its
//! [`WORKBENCHES`] are the engineers' workplaces, its one [`BAY`] is where a
//! robot is built and where a worn one waits for its overhaul, and its
//! [`RACK`] keeps the steel for the next robot (`sim::robots`).

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, LOT_H, LOT_W};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "robot factory";
/// An engineer's workplace: three, along the far wall.
pub const WORKBENCH: &str = "robot workbench";
pub const WORKBENCHES: usize = 3;
/// Where robots are built and overhauled: one.
pub const BAY: &str = "assembly bay";
/// The steel for the next robot.
pub const RACK: &str = "parts rack";
const WALL: &str = "block";
const FLOOR: &str = "floor";

/// One, on the lot whose middle is nearest the steel factory's. Draws nothing.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    whole_block: false,
    wanted: |_| true,
    rank: |siting, _, lot| {
        let steel = siting.the(super::steel_factory::NAME);
        let to = Point::new(steel.x + steel.w / 2, steel.y + steel.h / 2);
        lot.origin.offset(LOT_W / 2, LOT_H / 2).manhattan_distance(to)
    },
    build: |_, map, props, lot| robot_factory(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[WORKBENCH, BAY, RACK],
};

/// The robot factory, on `lot`: the whole lot, `WALL` round `FLOOR`, a plain
/// door in the middle of the lot's first street side. Laid out from the door
/// ([`Room`]): the row inside the door clear, the [`BAY`] against one side
/// wall and the [`RACK`] against the other two rows in, and the
/// [`WORKBENCHES`] along the far wall with a free cell between each.
fn robot_factory(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
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
    props.push(object(BAY, room.at(wide - 1, 2)));
    props.push(object(RACK, room.at(0, 2)));
    for across in [0, 2, 4] {
        props.push(object(WORKBENCH, room.at(across.min(wide - 1), deep - 1)));
    }
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One robot factory with its workbenches, bay and rack and nothing
    /// else, nobody's, and none of them anywhere else.
    #[test]
    fn a_district_has_one_robot_factory_with_three_workbenches_a_bay_and_a_rack() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let factory = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| factory.interior().contains(prop.cell()));
            assert!(
                inside.clone().all(|prop| [WORKBENCH, BAY, RACK].contains(&prop.kind.as_str())),
                "seed {seed}: the robot factory holds its own props only"
            );
            let everywhere = Rect::new(0, 0, WIDTH, HEIGHT);
            for (name, count) in [(WORKBENCH, WORKBENCHES), (BAY, 1), (RACK, 1)] {
                assert_eq!(props_in(map, factory, name), count, "seed {seed}: {name}");
                assert_eq!(props_in(map, everywhere, name), count, "seed {seed}: {name} elsewhere");
            }
            let properties = crate::sim::Properties::from_map(map);
            for cell in factory.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the robot factory is nobody's property");
            }
        }
    }
}
