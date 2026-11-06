//! The **church**: where the district prays. A lot building on the
//! residential lot nearest the district's middle that the playzone has not
//! taken, so nobody is far from it. Its [`ALTARS`] stand side by side along
//! the far wall — one long altar of five sections, each a cleric's workplace
//! (`sim::job`, `sim::religion`), led from the row in front of it — and its
//! [`PEW`]s in rows either side of an aisle from the door, prayed at by
//! anybody from beside them.

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, HEIGHT, LOT_H, LOT_W, WIDTH};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "church";
/// A section of the altar: a cleric's workplace.
pub const ALTAR: &str = "altar";
/// How many sections the altar has, and so how many clerics the district
/// has: one to each.
pub const ALTARS: usize = 5;
/// A pew, either side of the aisle.
pub const PEW: &str = "pew";
const WALL: &str = "wall brown";
const FLOOR: &str = "floor diagonal";

/// One, on the lot whose middle is nearest the middle of the district of the
/// lots still free when it is built — the playzone, built before it, takes
/// the nearest. Draws nothing.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    whole_block: false,
    wanted: |_| true,
    rank: |_, _, lot| lot.origin.offset(LOT_W / 2, LOT_H / 2).manhattan_distance(Point::new(WIDTH / 2, HEIGHT / 2)),
    build: |_, map, props, lot| church(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[ALTAR, PEW],
};

/// The church, on `lot`: the whole lot, `WALL` round `FLOOR`, a plain door —
/// nobody's property — in the middle of the lot's first street side. Laid out
/// from the door ([`Room`]):
///
/// * the row inside the door clear, to come in by;
/// * **pews** in the next row, and in the one two further on in a room deep
///   enough for it, in every place but the **aisle**, the columns the door
///   opens onto — each with the row behind and in front of it free to pray
///   from;
/// * the row in front of the altar clear: where the clerics stand;
/// * the [`ALTARS`] side by side along the far wall, as near its middle as
///   five fit.
///
/// Draws nothing from the RNG.
fn church(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
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
    for (across, depth) in layout(wide, deep).pews {
        props.push(object(PEW, room.at(across, depth)));
    }
    for (across, depth) in layout(wide, deep).altars {
        props.push(object(ALTAR, room.at(across, depth)));
    }
    walls
}

/// Where everything goes in a room `wide` across its door wall and `deep` in
/// from it, as `(across, depth)`.
struct Layout {
    pews: Vec<(i32, i32)>,
    altars: Vec<(i32, i32)>,
}

/// [`church`]'s plan. The aisle is the columns the door opens onto: the door
/// is [`DOOR_WIDTH`] wide in the middle of its wall, `wide + 2` long.
fn layout(wide: i32, deep: i32) -> Layout {
    let door = (wide + 2) / 2 - 2;
    let aisle = door..door + DOOR_WIDTH;
    let rows = if deep >= 6 { vec![1, 3] } else { vec![1] };
    let pews = rows
        .into_iter()
        .flat_map(|depth| (0..wide).filter(|across| !aisle.contains(across)).map(move |across| (across, depth)))
        .collect();
    let start = (wide - ALTARS as i32) / 2;
    let altars = (start..start + ALTARS as i32).map(|across| (across, deep - 1)).collect();
    Layout { pews, altars }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One church: its altars and pews and nothing else, nobody's, and no
    /// altar or pew anywhere else.
    #[test]
    fn a_district_has_one_church_with_five_altars_and_its_pews() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let church = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| church.interior().contains(prop.cell()));
            assert!(
                inside.clone().all(|prop| [ALTAR, PEW].contains(&prop.kind.as_str())),
                "seed {seed}: the church holds its altars and pews only"
            );
            assert_eq!(props_in(map, church, ALTAR), ALTARS, "seed {seed}");
            let everywhere = Rect::new(0, 0, WIDTH, HEIGHT);
            assert_eq!(props_in(map, everywhere, ALTAR), ALTARS, "seed {seed}: altars elsewhere");
            let pews = props_in(map, church, PEW);
            assert!(pews >= 4, "seed {seed}: {pews} pews");
            assert_eq!(props_in(map, everywhere, PEW), pews, "seed {seed}: pews elsewhere");
            let properties = crate::sim::Properties::from_map(map);
            for cell in church.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the church is nobody's property");
            }
        }
    }

    /// Every altar has the cell in front of it free, for its cleric, and every
    /// pew a free cell beside it to pray from — and nothing stands in the
    /// aisle or the row inside the door. (That the street reaches them is the
    /// sealed-off test's.)
    #[test]
    fn every_altar_and_pew_has_room_beside_it() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let church = district.the(NAME);
            let props = map.objects(ObjectLayer::Props);
            let free = |cell: Point| {
                church.interior().contains(cell) && map.is_passable(cell) && !props.iter().any(|prop| prop.cell() == cell)
            };
            for prop in props.iter().filter(|prop| church.interior().contains(prop.cell())) {
                let room = prop.cell().cardinal_neighbours().into_iter().filter(|&cell| free(cell)).count();
                assert!(room >= 1, "seed {seed}: the {} at {:?} has no free cell beside it", prop.kind.as_str(), prop.cell());
            }
        }
    }

    /// Both ways a lot can face the street: five altars on the far wall, the
    /// aisle clear from the door to the clerics' row, pews either side.
    #[test]
    fn the_plan_keeps_an_aisle_from_the_door_to_the_altar() {
        for (wide, deep) in [(LOT_W - 2, LOT_H - 2), (LOT_H - 2, LOT_W - 2)] {
            let plan = layout(wide, deep);
            assert_eq!(plan.altars.len(), ALTARS, "{wide}x{deep}");
            assert!(plan.altars.iter().all(|&(across, depth)| depth == deep - 1 && (0..wide).contains(&across)));
            let door = (wide + 2) / 2 - 2;
            for depth in 0..deep - 1 {
                for across in door..door + DOOR_WIDTH {
                    assert!(!plan.pews.contains(&(across, depth)), "{wide}x{deep}: a pew in the aisle at {across}, {depth}");
                }
            }
            assert!(plan.pews.iter().all(|&(_, depth)| depth > 0 && depth < deep - 2), "{wide}x{deep}: the door's and the clerics' rows clear");
            assert!(plan.pews.len() >= 3, "{wide}x{deep}");
        }
    }
}
