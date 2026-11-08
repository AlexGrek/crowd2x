//! The **charging stations**: three, on lots chosen at random, where robots
//! charge (`sim::robots`). Each has [`DOCKS`] charging docks side by side
//! along the far wall, with the row in front of them free for the robots
//! standing at them, so four charge at once.

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, LOT_H, LOT_W};
use crate::map::{Map, Object, Point};

/// What [`super::District::all`] calls them.
pub const NAME: &str = "charging station";
/// A charging dock: four to a station.
pub const DOCK: &str = "charging dock";
pub const DOCKS: usize = 4;
/// How many stations a district has.
pub const STATIONS: usize = 3;
const WALL: &str = "wall red";
const FLOOR: &str = "floor white";

/// Three, each on a lot of the shuffled order: **at random**, as far as the
/// seed goes. Draws nothing from the RNG.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: STATIONS,
    whole_block: false,
    wanted: |_| true,
    rank: |_, _, _| 0,
    build: |_, map, props, lot| charging_station(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[DOCK],
};

/// A charging station on `lot`: the whole lot, a plain door in the middle of
/// its first street side, the row inside it clear, and the [`DOCKS`] docks
/// along the far wall.
fn charging_station(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
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
    let start = (wide - DOCKS as i32) / 2;
    for across in start..start + DOCKS as i32 {
        props.push(object(DOCK, room.at(across, deep - 1)));
    }
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// Three stations of four docks each, nothing else in them, nobody's.
    #[test]
    fn a_district_has_three_charging_stations_of_four_docks() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let stations: Vec<Rect> = district.all(NAME).collect();
            assert_eq!(stations.len(), STATIONS, "seed {seed}");
            let properties = crate::sim::Properties::from_map(map);
            for station in &stations {
                let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| station.interior().contains(prop.cell()));
                assert!(inside.clone().all(|prop| prop.kind.as_str() == DOCK), "seed {seed}: a station holds docks only");
                assert_eq!(props_in(map, *station, DOCK), DOCKS, "seed {seed}");
                for cell in station.cells() {
                    assert_eq!(properties.of(cell), None, "seed {seed}: a station is nobody's property");
                }
            }
            assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), DOCK), STATIONS * DOCKS, "seed {seed}: docks elsewhere");
        }
    }
}
