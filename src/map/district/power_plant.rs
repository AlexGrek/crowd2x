//! The **power plant**: on a lot on the edge of the district, the one source
//! of its power. Its [`GENERATOR`]s feed the power line (`map::utilities`'
//! `SOURCES`), which runs from under each of them into the plant's walls and
//! round them to where the plant's line goes out to the main under the
//! street (`super::connect`); every house, shop and lot building is fed from
//! that main. Its [`CONTROL_PANEL`] is the power plant workers' workplace,
//! kept round the clock, three watches to it (`sim::job`).

use super::{build, object, tile, Lot, LotBuilding, Rect, Room, Side, DOOR, DOOR_WIDTH, LOT_H, LOT_W};
use crate::map::{Map, Object, Point};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "power plant";
/// What makes the district's power: one in each far corner, so
/// [`GENERATORS`] of them. A source on the power line, like a transformer.
pub const GENERATOR: &str = "generator";
pub const GENERATORS: usize = 2;
/// Where the plant is run from: a power plant worker's workplace, in the
/// middle of the far wall between the generators. Needs no power.
pub const CONTROL_PANEL: &str = "control panel";
pub const CONTROL_PANELS: usize = 1;
const WALL: &str = "block";
const FLOOR: &str = "tiles blue";

/// One, on the first edge lot left in the shuffled order. Draws nothing.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    wanted: |lot| lot.edge_sides().next().is_some(),
    rank: |_, _, _| 0,
    build: |_, map, props, lot| power_plant(map, props, lot),
    tiles: &[WALL, FLOOR],
    props: &[GENERATOR, CONTROL_PANEL],
};

/// The power plant, on `lot`: the whole lot, `WALL` round `FLOOR`, a plain
/// door — nobody's property — in the middle of the lot's first street side.
/// Laid out from the door ([`Room`]): a [`GENERATOR`] in each far corner and
/// the [`CONTROL_PANEL`] between them in the middle of the far wall, the
/// rest open floor.
///
/// **Nothing stands where the plant takes its line in.** That is the middle
/// of the inside of a wall facing a street (`super::service_entry`) — the
/// middle of the door's side, or of the lot's other street side — and the
/// generators are in the far corners, off every line from there to the lamp
/// in the middle of the room, so the wiring to it never meets the generators'
/// line. Draws nothing from the RNG.
fn power_plant(map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
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
        props.push(object(GENERATOR, room.at(across, deep - 1)));
    }
    props.push(object(CONTROL_PANEL, room.at(wide / 2, deep - 1)));
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::grid::power;
    use crate::map::utilities::{Network, Supply};
    use crate::map::ObjectLayer;

    /// One power plant on the edge: its generators and its control panel and
    /// nothing else, nobody's, none of them anywhere else, and a free cell
    /// beside the panel to stand at it from.
    #[test]
    fn a_district_has_one_power_plant_on_its_edge() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let plant = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| plant.interior().contains(prop.cell()));
            assert!(
                inside.clone().all(|prop| [GENERATOR, CONTROL_PANEL].contains(&prop.kind.as_str())),
                "seed {seed}: the power plant holds its generators and its control panel only"
            );
            for (name, count) in [(GENERATOR, GENERATORS), (CONTROL_PANEL, CONTROL_PANELS)] {
                assert_eq!(props_in(map, plant, name), count, "seed {seed}: {name}");
                assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), name), count, "seed {seed}: {name} elsewhere");
            }
            let properties = crate::sim::Properties::from_map(map);
            for cell in plant.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the power plant is nobody's property");
            }
            let on_the_edge = plant.x - STREET <= 1
                || WIDTH - STREET - 1 - plant.right() <= 1
                || plant.y - STREET <= 1
                || HEIGHT - STREET - 1 - plant.top() <= 1;
            assert!(on_the_edge, "seed {seed}: the power plant at {plant:?} is not on the edge");
            let panel = map.objects(ObjectLayer::Props).iter().find(|prop| prop.kind.as_str() == CONTROL_PANEL).unwrap().cell();
            assert!(
                panel.cardinal_neighbours().into_iter().any(|cell| plant.interior().contains(cell) && map.is_passable(cell)),
                "seed {seed}: nowhere to stand at the control panel at {panel:?}"
            );
        }
    }

    /// The plant is what powers the district: its generators are the only
    /// source, they reach the main, and with them gone nothing has power —
    /// not a lamp, a fridge or a computer.
    #[test]
    fn the_power_plant_powers_the_whole_district_and_nothing_else_does() {
        for seed in SEEDS {
            let district = generate(seed);
            let mut map = district.map.clone();
            let sources: Vec<&str> = map
                .objects(ObjectLayer::Props)
                .iter()
                .map(|prop| prop.kind.as_str())
                .filter(|&name| crate::map::utilities::source_of(name) == Some(crate::map::utilities::Utility::Power))
                .collect();
            assert_eq!(sources, [GENERATOR; GENERATORS], "seed {seed}: the generators and no transformer");

            let supply = Supply::from_map(&map);
            let generators: Vec<Object> =
                map.objects(ObjectLayer::Props).iter().filter(|prop| prop.kind.as_str() == GENERATOR).cloned().collect();
            for generator in &generators {
                assert_eq!(map.grid(crate::map::GridLayer::Power, generator.cell()), power::LINE, "seed {seed}: on its line");
            }
            let consumers = supply.consumers(&map).filter(|(_, utility, _)| *utility == crate::map::utilities::Utility::Power).count();
            assert!(consumers > 3 * HOUSES, "seed {seed}: {consumers}");

            for generator in &generators {
                map.remove_object(ObjectLayer::Props, generator);
            }
            let dark = Supply::from_map(&map);
            assert!(
                map.size().points().all(|cell| !dark.is_live(Network::Line, cell) && !dark.is_live(Network::Wiring, cell)),
                "seed {seed}: something has power with the plant's generators gone"
            );
        }
    }
}
