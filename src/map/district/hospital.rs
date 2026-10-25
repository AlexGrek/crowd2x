//! Four patient beds, two nurse stations, a fridge and a toilet.
use super::{DOOR, DOOR_WIDTH, LOT_H, LOT_W, LotBuilding, Rect, Room, Side, build, object, tile};
use crate::map::Point;

pub const NAME: &str = "hospital";
pub const BEDS: usize = 4;
pub const STATIONS: usize = 2;
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    wanted: |_| true,
    rank: |_, _, _| 0,
    tiles: &["wall purple", "floor white"],
    props: &["hospital bed", "nurse station", "toilet", "fridge"],
    build: |_, map, props, lot| {
        let walls = Rect::new(lot.origin.x, lot.origin.y, LOT_W, LOT_H);
        build(map, walls, "wall purple", "floor white");
        let side = lot.outer[0];
        for i in 0..DOOR_WIDTH {
            let cell = match side {
                Side::South | Side::North => Point::new(
                    walls.x + walls.w / 2 - 1 + i,
                    if side == Side::South { walls.y } else { walls.top() },
                ),
                Side::West | Side::East => Point::new(
                    if side == Side::West { walls.x } else { walls.right() },
                    walls.y + walls.h / 2 - 1 + i,
                ),
            };
            map.set_terrain(cell, tile("floor white"));
            props.push(object(DOOR, cell));
        }
        let inside = walls.interior();
        let (wide, deep) = match side {
            Side::South | Side::North => (inside.w, inside.h),
            _ => (inside.h, inside.w),
        };
        let room = Room { walls, side, wide, deep };
        for across in [0, wide - 1] {
            props.push(object("nurse station", room.at(across, 0)));
            for depth in [deep - 3, deep - 1] {
                props.push(object("hospital bed", room.at(across, depth)));
            }
        }
        props.push(object("fridge", room.at(wide / 2 - 1, deep - 1)));
        props.push(object("toilet", room.at(wide / 2, deep - 1)));
        walls
    },
};

#[cfg(test)]
mod tests {
    use super::super::{generate, tests::SEEDS};
    use super::*;
    use crate::map::ObjectLayer;
    #[test]
    fn a_hospital_has_four_accessible_beds_two_stations_and_essential_facilities() {
        for seed in SEEDS {
            let district = generate(seed);
            let room = district.the(NAME);
            let props = district.map.objects(ObjectLayer::Props);
            for (name, count) in [("hospital bed", BEDS), ("nurse station", STATIONS), ("fridge", 1), ("toilet", 1)] {
                let found: Vec<_> = props
                    .iter()
                    .filter(|p| room.interior().contains(p.cell()) && p.kind.as_str() == name)
                    .collect();
                assert_eq!(found.len(), count, "seed {seed}: {name}");
                for prop in found {
                    assert!(
                        prop.cell().cardinal_neighbours().iter().any(|&c| district.map.is_passable(c)),
                        "{name} inaccessible"
                    );
                }
            }
        }
    }
}
