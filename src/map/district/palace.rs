//! The **palace**: on the lot nearest the middle of the district, the seat of
//! its king. Its [`THRONE`] is the king's workplace, and there is one, so a
//! district has one king (`sim::job`), who holds court at it.

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, HEIGHT, LOT_H, LOT_W, WIDTH};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "palace";
/// Where the king holds court: in the middle of the far wall, facing the
/// door. Needs no power.
pub const THRONE: &str = "throne";
pub const THRONES: usize = 1;
const WALL: &str = "wall red";
const FLOOR: &str = "floor white";

/// One, on the lot whose middle is nearest the district's. Draws nothing.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    wanted: |_| true,
    rank: |_, _, lot| lot.origin.offset(LOT_W / 2, LOT_H / 2).manhattan_distance(middle_of_the_district()),
    build: |_, map, props, lot| palace(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[THRONE],
};

/// The cell in the middle of the district, which the palace is built nearest.
fn middle_of_the_district() -> Point {
    Point::new(WIDTH / 2, HEIGHT / 2)
}

/// The palace, on `lot`: the whole lot, `WALL` round `FLOOR`, a plain door —
/// nobody's property — in the middle of the lot's first street side, and the
/// [`THRONE`] in the middle of the far wall, across the hall from it ([`Room`]),
/// the rest open floor. Draws nothing from the RNG.
fn palace(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
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
    props.push(object(THRONE, room.at(wide / 2, deep - 1)));
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One palace: its throne and nothing else, nobody's, the throne with a
    /// free cell beside it to hold court from, none anywhere else, and on no
    /// lot farther from the middle of the district than a house or a park it
    /// could have had.
    #[test]
    fn a_district_has_one_palace_in_its_middle_with_a_throne_in_it() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let palace = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| palace.interior().contains(prop.cell()));
            assert!(inside.clone().all(|prop| prop.kind.as_str() == THRONE), "seed {seed}: the palace holds its throne only");
            assert_eq!(props_in(map, palace, THRONE), THRONES, "seed {seed}");
            assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), THRONE), THRONES, "seed {seed}: a throne elsewhere");
            let properties = crate::sim::Properties::from_map(map);
            for cell in palace.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the palace is nobody's property");
            }
            let throne = inside.clone().next().unwrap().cell();
            assert!(
                throne.cardinal_neighbours().into_iter().any(|cell| palace.interior().contains(cell) && map.is_passable(cell)),
                "seed {seed}: nowhere to stand at the throne at {throne:?}"
            );

            let middle = |rect: Rect| Point::new(rect.x + rect.w / 2, rect.y + rect.h / 2);
            let to_the_palace = middle(palace).manhattan_distance(middle_of_the_district());
            // Not the park in the shops' block: that lot was never on offer.
            let block = |rect: Rect| ((rect.x - STREET) / (BLOCK_W + STREET), (rect.y - STREET) / (BLOCK_H + STREET));
            let offered = |park: &Rect| district.shops.iter().all(|&shop| block(shop) != block(*park));
            let lots = district.houses.iter().map(|house| house.walls).chain(district.parks.iter().copied().filter(offered));
            for other in lots {
                assert!(
                    middle(other).manhattan_distance(middle_of_the_district()) + 2 >= to_the_palace,
                    "seed {seed}: the lot at {other:?} is nearer the middle than the palace at {palace:?}"
                );
            }
        }
    }
}
