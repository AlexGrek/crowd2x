//! The **farm**: where the farmers store the wheat they grow out in the
//! wasteland's good soil (`sim::fields`). On the lot facing the street round
//! the edge of the district that is nearest a plot of it
//! ([`soil_round`](super::soil_round)), so the walk out to the fields and
//! back with a pack of wheat is short; [`GRAIN_BINS`] against its side
//! walls, each a farmer's workplace, and so as many farmers.

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, LOT_H, LOT_W};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "farm";
/// Where a farmer stores its wheat, one farmer to a bin (`sim::job`).
pub const GRAIN_BIN: &str = "grain bin";
/// How many bins the farm has, four against each side wall, and so how many
/// farmers a district employs.
pub const GRAIN_BINS: usize = 8;
const WALL: &str = "wall brown";
const FLOOR: &str = "wood cracked";

/// One, on the edge lot nearest the good soil: ties to the first in the
/// shuffled order. The soil is the seed's wasteland's, so this draws
/// nothing and is the same every time.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    wanted: |lot| lot.edge_sides().next().is_some(),
    rank: |siting, _, lot| {
        let middle = lot.origin.offset(LOT_W / 2, LOT_H / 2);
        siting.soil.iter().map(|&plot| plot.manhattan_distance(middle)).min().unwrap_or(i32::MAX)
    },
    build: |_, map, props, lot| farm(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[GRAIN_BIN],
};

/// The farm, on `lot`: the whole lot, `WALL` round `FLOOR`, a plain door —
/// nobody's property — in the middle of the lot's first side facing the edge
/// street, towards the fields. Laid out from the door ([`Room`]): a
/// [`GRAIN_BIN`] against each side wall in each of the four rows past the
/// one inside the door, every one with its inward side free and the middle
/// of the room open floor. Draws nothing from the RNG.
fn farm(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
    let walls = Rect::new(lot.origin.x, lot.origin.y, LOT_W, LOT_H);
    build(map, walls, WALL, FLOOR);
    let side = lot.edge_sides().next().expect("the farm is on the edge");
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
    for depth in 1..=(GRAIN_BINS / 2) as i32 {
        for across in [0, wide - 1] {
            props.push(object(GRAIN_BIN, room.at(across, depth)));
        }
    }
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{doors_of, props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::{is_plot, ObjectLayer};

    /// One farm: its grain bins and nothing else, nobody's, on the edge, and
    /// none of them anywhere else.
    #[test]
    fn a_district_has_one_farm_of_grain_bins_on_the_edge() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let farm = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| farm.interior().contains(prop.cell()));
            assert!(inside.clone().all(|prop| prop.kind.as_str() == GRAIN_BIN), "seed {seed}: the farm holds grain bins only");
            assert_eq!(inside.count(), GRAIN_BINS, "seed {seed}");
            assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), GRAIN_BIN), GRAIN_BINS, "seed {seed}: bins nowhere else");
            let properties = crate::sim::Properties::from_map(map);
            for cell in farm.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the farm is nobody's property");
            }
            // Every bin has a free side inside, to be used from.
            for bin in map.objects(ObjectLayer::Props).iter().filter(|prop| prop.kind.as_str() == GRAIN_BIN) {
                assert!(bin.cell().cardinal_neighbours().iter().any(|&side| farm.interior().contains(side) && map.is_passable(side)), "seed {seed}: a bin at {:?} with nowhere to stand", bin.cell());
            }
            // Its door faces the street round the edge.
            let doors: Vec<Point> = doors_of(map).into_iter().filter(|&(cell, _)| farm.contains(cell)).map(|(cell, _)| cell).collect();
            assert_eq!(doors.len(), DOOR_WIDTH as usize, "seed {seed}");
            for door in doors {
                let outside = door.cardinal_neighbours().into_iter().find(|&cell| !farm.contains(cell)).unwrap();
                let edge_street = outside.x < STREET || outside.x >= WIDTH - STREET || outside.y < STREET || outside.y >= HEIGHT - STREET;
                assert!(edge_street, "seed {seed}: the farm's door at {door:?} opens onto {outside:?}");
            }
        }
    }

    /// The good soil round a district is enough for every farmer's field
    /// many times over, and the farm is the edge lot nearest it: no house or
    /// park on the edge — the lots there were to be had when the farm, built
    /// last, took its own — is nearer.
    #[test]
    fn the_farm_stands_on_the_edge_lot_nearest_the_good_soil() {
        let fields = GRAIN_BINS * crate::sim::fields::PLOTS;
        let mut nearest = Vec::new();
        for seed in SEEDS {
            let district = generate(seed);
            let soil = soil_round(seed);
            // 282 to 2243 over the seeds.
            assert!(soil.len() >= 3 * fields, "seed {seed}: {} plots of good soil", soil.len());
            let to_soil = |rect: Rect| {
                let middle = Point::new(rect.x + rect.w / 2, rect.y + rect.h / 2);
                soil.iter().map(|&plot| plot.manhattan_distance(middle)).min().unwrap()
            };
            let farm = district.the(NAME);
            let on_the_edge = |rect: Rect| {
                rect.x - STREET <= 1 || WIDTH - STREET - 1 - rect.right() <= 1 || rect.y - STREET <= 1 || HEIGHT - STREET - 1 - rect.top() <= 1
            };
            let others = district.houses.iter().map(|house| house.walls).chain(district.parks.iter().copied()).filter(|&rect| on_the_edge(rect));
            for other in others {
                assert!(to_soil(other) + 2 >= to_soil(farm), "seed {seed}: the lot at {other:?} is nearer the soil than the farm at {farm:?}");
            }
            nearest.push(to_soil(farm));
            for plot in &soil {
                assert!(is_plot(*plot) && district.map.can_sow(*plot), "seed {seed}: {plot:?} is not a plot that can be sown");
            }
        }
        // Measured: 10 to 12 cells from the middle of the farm's lot for 58
        // of the 64 seeds, 33 at most — just past the wall.
        nearest.sort_unstable();
        assert!(nearest[nearest.len() / 2] <= 12 && nearest[nearest.len() - 1] <= 40, "from the farm to good soil: {nearest:?}");
    }
}
