//! Light switches: [`LightSwitches`], which room each one switches and
//! whether it is on.
//!
//! A [`LIGHT_SWITCH`] is a prop on a wall (`map::mount`) that switches the
//! lamps of the room it is in. **The room is derived, never authored**, like
//! a property: the roofed floor joined to the switch's cell, up to walls,
//! doorways and open sky. So a switch by a house's door is the house's, and
//! one in the bank's lobby is the lobby's and not the restroom's behind a door.
//! Furniture stands *in* a room, so a bed does not cut one in two. A switch
//! under open sky is in no room and switches nothing: it is for inside.
//!
//! **Every switch in one room is one switch.** Flipping any of them switches
//! the room, and all of them read the same — two switches at a hall's two
//! doors work the way a hall's do, rather than one of them having to be found
//! and flipped first.
//!
//! What is switched is the lamps and only the lamps
//! (`map::utilities::SWITCHED`): the room's fridge and computer are on the
//! same wiring and stay on. A room switched off reaches the supply as the
//! cells whose lamps are off ([`LightSwitches::dark_cells`], handed to
//! `Supply::with_lights_off`), so the lighting, the x-ray and the readouts
//! follow a light switch exactly as they follow a box. **Every switch is on
//! when a world begins** — which switches are off is the simulation's state,
//! not the map's, the way a box's is — so a map whose lamps were lit before
//! it had switches is lit the same with them.

use crate::map::utilities::LIGHT_SWITCH;
use crate::map::{Map, ObjectLayer, Point};

use super::door::is_door;

/// Every light switch on a map, the room each one switches, and which rooms
/// are switched off. Built once with the world; a switch is flipped by the
/// spawn pass (`GameState::switch_light`), never by a tick.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct LightSwitches {
    /// Every switch's cell, sorted, and the index of its room in `rooms`.
    switches: Vec<(Point, usize)>,
    /// Each room's cells, sorted. A switch in no room has one of its own
    /// with no cells, so it still flips and still reads on or off.
    rooms: Vec<Vec<Point>>,
    /// Whether each room's lights are switched off, by the index in `rooms`.
    off: Vec<bool>,
}

impl LightSwitches {
    /// Every light switch on `map`, each with its room, all switched on. One
    /// flood per room, so it is the cost of building a world, not a tick.
    pub fn from_map(map: &Map) -> LightSwitches {
        let mut cells: Vec<Point> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|object| object.kind.as_str() == LIGHT_SWITCH)
            .map(|object| object.cell())
            .collect();
        cells.sort_unstable();
        cells.dedup();
        let mut doors: Vec<Point> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|object| is_door(object.kind.as_str()))
            .map(|object| object.cell())
            .collect();
        doors.sort_unstable();

        // Inside: under a roof, on floor, and not a doorway.
        let inside = |cell: Point| {
            map.has_ceiling(cell)
                && map.terrain(cell).is_some_and(|tile| tile.is_passable())
                && doors.binary_search(&cell).is_err()
        };
        let mut switches = LightSwitches::default();
        for cell in cells {
            // A second switch in a room already found is that room's.
            let known = switches.rooms.iter().position(|room| room.binary_search(&cell).is_ok());
            let room = known.unwrap_or_else(|| {
                switches.rooms.push(if inside(cell) { flood(map, cell, &inside) } else { Vec::new() });
                switches.off.push(false);
                switches.rooms.len() - 1
            });
            switches.switches.push((cell, room));
        }
        switches
    }

    /// The room of the switch in `at`, by its index, or `None` for a cell
    /// with no switch.
    fn room(&self, at: Point) -> Option<usize> {
        let index = self.switches.binary_search_by_key(&at, |&(cell, _)| cell).ok()?;
        Some(self.switches[index].1)
    }

    /// Whether there is a light switch in `at`.
    pub fn has(&self, at: Point) -> bool {
        self.room(at).is_some()
    }

    /// Whether the switch in `at` is on. `false` where there is none.
    pub fn is_on(&self, at: Point) -> bool {
        self.room(at).is_some_and(|room| !self.off[room])
    }

    /// Switch the room of the switch in `at` on or off — every switch in it.
    /// `None` when there is no switch there; otherwise whether anything
    /// changed, which it does not when it was already that way.
    pub fn switch(&mut self, at: Point, on: bool) -> Option<bool> {
        let room = self.room(at)?;
        let changed = self.off[room] == on;
        self.off[room] = !on;
        Some(changed)
    }

    /// The cells the switch in `at` switches the lamps of: its room, sorted.
    /// Empty for a switch in no room, or no switch.
    pub fn room_of(&self, at: Point) -> &[Point] {
        self.room(at).map_or(&[], |room| &self.rooms[room])
    }

    /// Every cell whose lamps are switched off, room by room.
    pub fn dark_cells(&self) -> impl Iterator<Item = Point> + '_ {
        self.rooms.iter().zip(&self.off).filter(|&(_, &off)| off).flat_map(|(room, _)| room.iter().copied())
    }

    /// Every switch's cell, sorted.
    pub fn cells(&self) -> impl Iterator<Item = Point> + '_ {
        self.switches.iter().map(|&(cell, _)| cell)
    }

    pub fn len(&self) -> usize {
        self.switches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.switches.is_empty()
    }
}

/// The cells reached from `start` through `inside` ones, sorted.
fn flood(map: &Map, start: Point, inside: &dyn Fn(Point) -> bool) -> Vec<Point> {
    let size = map.size();
    let mut seen = vec![false; size.area()];
    let Some(index) = size.index_of(start) else {
        return Vec::new();
    };
    seen[index] = true;
    let mut room = vec![start];
    let mut next = 0;
    while next < room.len() {
        for neighbour in room[next].cardinal_neighbours() {
            if let Some(index) = size.index_of(neighbour)
                && !seen[index]
                && inside(neighbour)
            {
                seen[index] = true;
                room.push(neighbour);
            }
        }
        next += 1;
    }
    room.sort_unstable();
    room
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR, WALL};
    use crate::sim::testing::prop_at;

    /// Two roofed rooms side by side, walls included: a hall from (1, 1) to
    /// (5, 4) and a closet from (7, 1) to (8, 4), a door between them at
    /// (6, 2), and open sky round both. A switch in each by the far wall.
    fn flat() -> Map {
        let mut map = Map::new(Size::new(14, 8), FLOOR);
        for y in 0..=5 {
            for x in 0..=9 {
                map.set_ceiling(Point::new(x, y), true);
                if x == 0 || x == 6 || x == 9 || y == 0 || y == 5 {
                    map.set_terrain(Point::new(x, y), WALL);
                }
            }
        }
        map.set_terrain(Point::new(6, 2), FLOOR);
        prop_at(&mut map, "door", Point::new(6, 2));
        prop_at(&mut map, LIGHT_SWITCH, Point::new(2, 4));
        prop_at(&mut map, LIGHT_SWITCH, Point::new(8, 4));
        prop_at(&mut map, "bed 1", Point::new(3, 2));
        map
    }

    #[test]
    fn a_switch_switches_the_roofed_floor_round_it_up_to_walls_and_doors() {
        let switches = LightSwitches::from_map(&flat());
        assert_eq!(switches.len(), 2);
        let hall = switches.room_of(Point::new(2, 4));
        assert_eq!(hall.len(), 5 * 4, "the whole hall, the bed's cell too");
        assert!(hall.contains(&Point::new(3, 2)), "a bed does not cut a room in two");
        assert!(!hall.contains(&Point::new(6, 2)), "the doorway is nobody's room");
        assert!(!hall.contains(&Point::new(0, 2)), "nor is a wall");
        let closet = switches.room_of(Point::new(8, 4));
        assert_eq!(closet.len(), 2 * 4);
        assert!(closet.iter().all(|cell| !hall.contains(cell)));
        assert!(switches.room_of(Point::new(4, 4)).is_empty(), "no switch there");
    }

    #[test]
    fn a_switch_under_open_sky_switches_nothing_but_still_flips() {
        let mut map = flat();
        prop_at(&mut map, LIGHT_SWITCH, Point::new(10, 2));
        let mut switches = LightSwitches::from_map(&map);
        let outside = Point::new(10, 2);
        assert!(switches.has(outside) && switches.room_of(outside).is_empty());
        assert_eq!(switches.switch(outside, false), Some(true));
        assert!(!switches.is_on(outside));
        assert_eq!(switches.dark_cells().count(), 0);
    }

    #[test]
    fn a_room_switched_off_is_dark_and_only_that_room() {
        let mut switches = LightSwitches::from_map(&flat());
        let (hall, closet) = (Point::new(2, 4), Point::new(8, 4));
        assert!(switches.is_on(hall) && switches.is_on(closet), "a world begins with its lights on");
        assert_eq!(switches.dark_cells().count(), 0);

        assert_eq!(switches.switch(hall, false), Some(true));
        assert_eq!(switches.switch(hall, false), Some(false), "it was already off");
        assert!(!switches.is_on(hall) && switches.is_on(closet));
        let dark: Vec<Point> = switches.dark_cells().collect();
        assert_eq!(dark, switches.room_of(hall));

        assert_eq!(switches.switch(hall, true), Some(true));
        assert_eq!(switches.dark_cells().count(), 0);
        assert_eq!(switches.switch(Point::new(4, 4), false), None, "no switch there");
    }

    #[test]
    fn two_switches_in_one_room_are_one_switch() {
        let mut map = flat();
        prop_at(&mut map, LIGHT_SWITCH, Point::new(5, 1));
        let mut switches = LightSwitches::from_map(&map);
        let (near, far) = (Point::new(2, 4), Point::new(5, 1));
        assert_eq!(switches.room_of(near), switches.room_of(far));
        switches.switch(far, false);
        assert!(!switches.is_on(near), "flipped at the other door");
        assert_eq!(switches.dark_cells().count(), 5 * 4, "and the room counted once");
        switches.switch(near, true);
        assert!(switches.is_on(far));
    }

    #[test]
    fn the_same_map_finds_the_same_rooms() {
        assert_eq!(LightSwitches::from_map(&flat()), LightSwitches::from_map(&flat()));
    }
}
