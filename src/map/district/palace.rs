//! The **palace**: the whole of the block in the middle of the district, the
//! seat of its king, its door onto the main street. Its [`THRONE`] is the
//! king's workplace, and there is one, so a district has one king
//! (`sim::job`), who holds court at it.
//!
//! Not a lot building: it is a block of its own, taken out before the bank
//! and the shops are given theirs, so it is in the middle of every district
//! whatever the seed.

use super::{block, build, object, tile, Rect, Room, Side, BLOCKS_X, BLOCKS_Y, DOOR, DOOR_WIDTH};
use crate::map::{Map, Object, Point};

/// What it is called.
pub const NAME: &str = "palace";
/// Where the king holds court: in the middle of the far wall, facing the
/// door down the length of the hall. Needs no power.
pub const THRONE: &str = "throne";
pub const THRONES: usize = 1;
/// The block it stands on: the one in the middle of the district, which
/// has the main street along its front (`super::MAIN`).
pub const BLOCK: (i32, i32) = (BLOCKS_X / 2, BLOCKS_Y / 2);
/// Cells between two of its ceiling lamps, each way: a ceiling lamp reaches
/// four and a half, so their pools meet across the hall.
pub const LAMP_SPACING: i32 = 5;
const WALL: &str = "wall red";
const FLOOR: &str = "floor white";
/// The runner from the door to the throne.
const CARPET: &str = "floor red";
/// Every tile it paints with, for the catalogue test.
pub const TILES: &[&str] = &[WALL, FLOOR, CARPET];
/// Every prop it places but its doors, for the catalogue test.
pub const PROPS: &[&str] = &[THRONE];

/// The palace: the whole of the middle block, `WALL` round `FLOOR`, a plain
/// door — nobody's property — in the middle of the wall onto the main
/// street, and the [`THRONE`] in the middle of the far wall across the hall
/// from it ([`Room`]), a `CARPET` two cells wide running from the door to
/// it; the rest open floor, lit by a grid of lamps (`super::roof_and_light`).
/// Draws nothing from the RNG.
pub(super) fn palace(map: &mut Map, props: &mut Vec<Object>) -> Rect {
    let walls = block(BLOCK.0, BLOCK.1);
    build(map, walls, WALL, FLOOR);
    // The main street runs along the bottom of the middle row of blocks.
    let side = Side::South;
    let door: Vec<Point> = (0..DOOR_WIDTH).map(|i| Point::new(walls.x + walls.w / 2 - 1 + i, walls.y)).collect();
    let inside = walls.interior();
    let room = Room { walls, side, wide: inside.w, deep: inside.h };
    let carpet = tile(CARPET);
    for &leaf in &door {
        map.set_terrain(leaf, carpet);
        props.push(object(DOOR, leaf));
        for depth in 0..room.deep {
            map.set_terrain(room.at(leaf.x - inside.x, depth), carpet);
        }
    }
    props.push(object(THRONE, room.at(room.wide / 2, room.deep - 1)));
    walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{doors_of, props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// One palace, the whole of the block in the middle of the district: its
    /// throne and nothing else, nobody's, the throne with a free cell beside
    /// it to hold court from, none anywhere else, and its door onto the main
    /// street.
    #[test]
    fn a_district_has_one_palace_filling_its_middle_block_with_a_throne_in_it() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let palace = district.palace;
            assert_eq!(palace, block(BLOCK.0, BLOCK.1), "seed {seed}: the palace is the whole of the middle block");
            // In the middle, but for the main street's extra width below it.
            let middle = Point::new(palace.x + palace.w / 2, palace.y + palace.h / 2);
            let off = middle.manhattan_distance(Point::new(WIDTH / 2, HEIGHT / 2));
            assert!(off <= (MAIN_STREET - STREET) / 2 + 1, "seed {seed}: the palace at {palace:?} is {off} off the middle");
            for walls in district.buildings().into_iter().filter(|&walls| walls != palace) {
                assert!(walls.cells().all(|cell| !palace.contains(cell)), "seed {seed}: {walls:?} stands in the palace");
            }

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

            // The door opens onto the main street's pavement, and the throne
            // faces it across the hall, the carpet running between.
            let doors: Vec<Point> = doors_of(map).into_iter().filter(|&(cell, _)| palace.contains(cell)).map(|(cell, _)| cell).collect();
            assert_eq!(doors.len(), DOOR_WIDTH as usize, "seed {seed}");
            let main = across(MAIN);
            for &door in &doors {
                let outside = door.offset(0, -1);
                assert!(main.contains(outside.y), "seed {seed}: the palace's door at {door:?} is not onto the main street");
                assert_eq!(map.terrain(outside), Some(tile(PAVEMENT)), "seed {seed}");
            }
            assert!(doors.iter().any(|door| door.x == throne.x), "seed {seed}: the throne at {throne:?} is not across from the door");
            for y in palace.y..=throne.y {
                assert_eq!(map.terrain(Point::new(throne.x, y)), Some(tile(CARPET)), "seed {seed}: the carpet to the throne at {y}");
            }
        }
    }

    /// The palace's hall is lit all over: a grid of ceiling lamps, every
    /// cell of the floor within a lamp's reach.
    #[test]
    fn the_palace_is_lit_by_a_grid_of_lamps() {
        let radius = crate::lighting::scene::emission(HOUSE_LAMP).expect("a ceiling lamp gives off light").radius;
        let district = generate(7);
        let palace = district.palace;
        let lamps: Vec<Point> = district
            .map
            .objects(ObjectLayer::Lamps)
            .iter()
            .filter(|lamp| lamp.kind.as_str() == HOUSE_LAMP && palace.interior().contains(lamp.cell()))
            .map(|lamp| lamp.cell())
            .collect();
        assert!(lamps.len() >= 9, "{} lamps", lamps.len());
        for cell in palace.interior().cells() {
            let near = lamps.iter().any(|lamp| (((lamp.x - cell.x).pow(2) + (lamp.y - cell.y).pow(2)) as f32).sqrt() < radius);
            assert!(near, "{cell:?} is beyond every lamp in the palace");
        }
    }
}
