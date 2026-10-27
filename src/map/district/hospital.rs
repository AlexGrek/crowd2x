//! Eight patient beds, two nurse stations, a fridge and a toilet.
//!
//! The beds line both side walls from the row past the nurse stations, each
//! with the aisle beside it ([`beds`]): about one bite victim a night,
//! healing a day or two, outgrew four (the `lab` skill).
use super::{DOOR, DOOR_WIDTH, LOT_H, LOT_W, LotBuilding, Rect, Room, Side, build, object, tile};
use crate::map::Point;

pub const NAME: &str = "hospital";
pub const BEDS: usize = 8;
pub const STATIONS: usize = 2;
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    whole_block: false,
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
        let (fridge, toilet) = ((wide / 2 - 1, deep - 1), (wide / 2, deep - 1));
        let stations = [(0, 0), (wide - 1, 0)];
        for (across, depth) in stations {
            props.push(object("nurse station", room.at(across, depth)));
        }
        for (across, depth) in beds(wide, deep, &[stations[0], stations[1], fridge, toilet]) {
            props.push(object("hospital bed", room.at(across, depth)));
        }
        props.push(object("fridge", room.at(fridge.0, fridge.1)));
        props.push(object("toilet", room.at(toilet.0, toilet.1)));
        walls
    },
};

/// Where [`BEDS`] beds go in a room `wide` across its door wall and `deep`
/// in from it, round the props already `placed` (as `(across, depth)`): down
/// both side walls, nearest the stations first, each kept only if the room
/// stays whole with it — every free cell still reached from the doorway and
/// every prop still with a free side to be used from. A room laid out either
/// way round (6 by 5 or 5 by 6) takes all eight; in the deeper one the corner
/// beside the fridge is skipped for the one past it.
fn beds(wide: i32, deep: i32, placed: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut taken: Vec<(i32, i32)> = placed.to_vec();
    let mut beds = Vec::new();
    for depth in 1..deep {
        for across in [0, wide - 1] {
            if beds.len() == BEDS || taken.contains(&(across, depth)) {
                continue;
            }
            taken.push((across, depth));
            if whole(wide, deep, &taken) {
                beds.push((across, depth));
            } else {
                taken.pop();
            }
        }
    }
    beds
}

/// Whether a room with `taken` cells is still one room: everything free
/// reached from the doorway's two cells, and everything taken beside
/// something reached.
fn whole(wide: i32, deep: i32, taken: &[(i32, i32)]) -> bool {
    let free = |cell: (i32, i32)| {
        (0..wide).contains(&cell.0) && (0..deep).contains(&cell.1) && !taken.contains(&cell)
    };
    let sides = |(a, d): (i32, i32)| [(a - 1, d), (a + 1, d), (a, d - 1), (a, d + 1)];
    let mut reached: Vec<(i32, i32)> = [(wide / 2 - 1, 0), (wide / 2, 0)].into_iter().filter(|&c| free(c)).collect();
    let mut next = 0;
    while next < reached.len() {
        for side in sides(reached[next]) {
            if free(side) && !reached.contains(&side) {
                reached.push(side);
            }
        }
        next += 1;
    }
    let cells = (0..wide).flat_map(|a| (0..deep).map(move |d| (a, d)));
    cells.clone().filter(|&c| free(c)).all(|c| reached.contains(&c))
        && taken.iter().all(|&c| sides(c).iter().any(|s| reached.contains(s)))
}

#[cfg(test)]
mod tests {
    use super::super::{generate, tests::SEEDS};
    use super::*;
    use crate::map::ObjectLayer;

    #[test]
    fn eight_beds_fit_a_room_either_way_round_and_leave_it_whole() {
        for (wide, deep) in [(6, 5), (5, 6)] {
            let placed = [(0, 0), (wide - 1, 0), (wide / 2 - 1, deep - 1), (wide / 2, deep - 1)];
            let beds = beds(wide, deep, &placed);
            assert_eq!(beds.len(), BEDS, "{wide}x{deep}: {beds:?}");
            let mut taken = placed.to_vec();
            taken.extend(&beds);
            assert!(whole(wide, deep, &taken), "{wide}x{deep}");
        }
        // Two props either side of a corner shut it in.
        assert!(!whole(3, 3, &[(1, 2), (2, 1)]), "the far corner is shut in");
        assert!(whole(3, 3, &[(1, 2)]));
    }
    #[test]
    fn a_hospital_has_eight_accessible_beds_two_stations_and_essential_facilities() {
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
