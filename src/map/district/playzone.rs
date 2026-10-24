//! The **playzone**: an arcade on the residential lot nearest the middle of
//! the district, so nobody is far from the fun. Its [`CABINETS`] are somewhere
//! to have a quick go and stop being bored; its one [`PLAYZONE_COMPUTER`] is where the
//! playzone is run from, the workplace of its four administrators
//! (`sim::job`) — and **the cabinets work only while one of them is at it**
//! (`sim::playzone`).

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, HEIGHT, LOT_H, LOT_W, WIDTH};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "playzone";
/// The playzone's central computer, in the middle of the far wall: the
/// administrators' workplace, all four of them at the one computer.
pub const PLAYZONE_COMPUTER: &str = "playzone computer";
pub const COMPUTERS: usize = 1;
/// An arcade cabinet, along both side walls and in the far corners.
pub const CABINET: &str = "arcade cabinet";
pub const CABINETS: usize = 6;
const WALL: &str = "wall purple";
const FLOOR: &str = "floor colorful";

/// One, on the lot whose middle is nearest the middle of the district. Draws
/// nothing.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    wanted: |_| true,
    rank: |_, _, lot| lot.origin.offset(LOT_W / 2, LOT_H / 2).manhattan_distance(Point::new(WIDTH / 2, HEIGHT / 2)),
    build: |_, map, props, lot| playzone(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[PLAYZONE_COMPUTER, CABINET],
};

/// The playzone, on `lot`: the whole lot, `WALL` round `FLOOR`, a plain door —
/// nobody's property — in the middle of the lot's first street side. Laid out
/// from the door ([`Room`]): an arcade cabinet against each side wall in every
/// other row from the one inside the door to the far wall, so there is a free
/// cell beside every one to play it from; and the [`PLAYZONE_COMPUTER`] in the middle of
/// the far wall, with a free cell either side and in front, where its
/// administrator stands. Draws nothing from the RNG.
fn playzone(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
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
        for depth in (0..deep).step_by(2) {
            props.push(object(CABINET, room.at(across, depth)));
        }
    }
    props.push(object(PLAYZONE_COMPUTER, room.at(wide / 2, deep - 1)));
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One playzone: its computer and its cabinets and nothing else, nobody's,
    /// on the lot nearest the middle of the district, and none of them
    /// anywhere else.
    #[test]
    fn a_district_has_one_playzone_nearest_its_middle() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let playzone = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| playzone.interior().contains(prop.cell()));
            assert!(
                inside.clone().all(|prop| [PLAYZONE_COMPUTER, CABINET].contains(&prop.kind.as_str())),
                "seed {seed}: the playzone holds its computer and cabinets only"
            );
            for (name, count) in [(PLAYZONE_COMPUTER, COMPUTERS), (CABINET, CABINETS)] {
                assert_eq!(props_in(map, playzone, name), count, "seed {seed}: {name}");
                assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), name), count, "seed {seed}: {name} elsewhere");
            }
            let properties = crate::sim::Properties::from_map(map);
            for cell in playzone.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the playzone is nobody's property");
            }
            let middle = |rect: Rect| Point::new(rect.x + rect.w / 2, rect.y + rect.h / 2);
            let centre = Point::new(WIDTH / 2, HEIGHT / 2);
            let to_the_playzone = middle(playzone).manhattan_distance(centre);
            let lot_middles = district.houses.iter().map(|house| house.walls).chain(district.parks.iter().copied());
            for other in lot_middles {
                assert!(
                    middle(other).manhattan_distance(centre) + 2 >= to_the_playzone,
                    "seed {seed}: the lot at {other:?} is nearer the middle than the playzone at {playzone:?}"
                );
            }
        }
    }

    /// Every cabinet has a free cell inside beside it to play it from, and the
    /// computer two (one either side of it, where its administrator stands and
    /// somebody can walk past): a playzone nobody can use is worse than none.
    /// (That the street reaches them is the sealed-off test's.)
    #[test]
    fn every_cabinet_and_the_computer_has_room_beside_it() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let playzone = district.the(NAME);
            let props = map.objects(ObjectLayer::Props);
            let free = |cell: Point| {
                playzone.interior().contains(cell) && map.is_passable(cell) && !props.iter().any(|prop| prop.cell() == cell)
            };
            for prop in props.iter().filter(|prop| playzone.interior().contains(prop.cell())) {
                let room = prop.cell().cardinal_neighbours().into_iter().filter(|&cell| free(cell)).count();
                let wanted = if prop.kind.as_str() == PLAYZONE_COMPUTER { 2 } else { 1 };
                assert!(room >= wanted, "seed {seed}: the {} at {:?} has {room} free cells beside it", prop.kind.as_str(), prop.cell());
            }
        }
    }
}
