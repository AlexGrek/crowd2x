//! Whose place is whose: [`Properties`], the rooms behind locked doors.
//!
//! **A property is derived, never authored** — the same rule passability and
//! the power supply follow. A [`HOUSE_DOOR`] is a door only its owners may
//! open, and what it locks is the room behind it: the cells reached from its
//! inner side without going through a wall or another door. Which side is the
//! inner one is the side that is *enclosed* — a flood fill that stops within
//! [`LARGEST_PROPERTY`] cells — and of two enclosed sides, the smaller. The
//! street behind a house door floods the whole district, so it is never the
//! house. A house door with no enclosed side locks nothing, and is an
//! ordinary door.
//!
//! The leaves of one doorway, and every door into the same room, find the
//! same cells and so the same property. Ids are handed out in cell order of
//! the doors, so the same map numbers its properties the same way.
//!
//! **Owning one comes from owning a bed in it.** The spawn pass hands an
//! arrival a bed (`GameState::give_a_bed`) and with it the property that bed
//! stands in, as the body's [`Body::home`](crate::sim::Body::home) — a key,
//! carried by the unit, so nothing in a tick asks a shared table who owns
//! what. A property's furniture is its owners' alone: a brain never chooses a
//! fridge, toilet, bed or computer in somebody else's property
//! ([`Properties::may_use`]), and its routes never go through a door it holds
//! no key to (`sim::door::Doors::lets_through`).

use std::num::NonZeroU16;

use crate::map::{Map, ObjectLayer, Point, Size};

use super::door::HOUSE_DOOR;

/// The most cells a room behind a house door may have. Far more than any
/// house, and far fewer than a street network — which is what tells the
/// inside of a door from its outside.
pub const LARGEST_PROPERTY: usize = 1024;

/// One property. Never zero, so `Option<PropertyId>` is two bytes and fits in
/// the padding of a [`crate::sim::Body`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PropertyId(NonZeroU16);

impl PropertyId {
    /// The `n`th property, counting from one.
    pub fn new(n: u16) -> Option<PropertyId> {
        NonZeroU16::new(n).map(PropertyId)
    }

    pub fn get(self) -> u16 {
        self.0.get()
    }
}

impl std::fmt::Display for PropertyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "property {}", self.0)
    }
}

/// Which cells belong to which property: a `u16` per cell, zero for nobody's.
/// Built once with the world, read from every thread.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Properties {
    /// `None` only for the empty default, which is nobody's anywhere.
    size: Option<Size>,
    cells: Vec<u16>,
    count: u16,
}

impl Properties {
    /// Every room on `map` locked behind a [`HOUSE_DOOR`], the door cells
    /// included. Terrain only: furniture stands *in* a room, so a bed's cell
    /// is as much the house's as the floor beside it.
    pub fn from_map(map: &Map) -> Properties {
        let size = map.size();
        let mut properties = Properties {
            size: Some(size),
            cells: vec![0; size.area()],
            count: 0,
        };
        let mut doors: Vec<Point> = Vec::new();
        let mut locked: Vec<Point> = Vec::new();
        for object in map.objects(ObjectLayer::Props) {
            if super::door::is_door(object.kind.as_str()) {
                doors.push(object.cell());
                if object.kind.as_str() == HOUSE_DOOR {
                    locked.push(object.cell());
                }
            }
        }
        doors.sort_unstable();
        locked.sort_unstable();
        locked.dedup();

        let open = |cell: Point| {
            map.terrain(cell).is_some_and(|terrain| terrain.is_passable()) && doors.binary_search(&cell).is_err()
        };
        for door in locked {
            let inside = door
                .cardinal_neighbours()
                .into_iter()
                .filter(|&side| open(side))
                .filter_map(|side| enclosed(size, side, &open))
                .min_by_key(|room| (room.len(), room[0]));
            let Some(room) = inside else {
                continue;
            };
            let index = |cell: Point| size.index_of(cell).expect("flooded on the map");
            let id = match properties.cells[index(room[0])] {
                0 => {
                    properties.count += 1;
                    for &cell in &room {
                        properties.cells[index(cell)] = properties.count;
                    }
                    properties.count
                }
                // Another leaf of the same doorway, or a second door into
                // the same room: it is already somebody's.
                id => id,
            };
            properties.cells[index(door)] = id;
        }
        properties
    }

    /// The property `cell` belongs to, or `None` for one that is nobody's —
    /// and for a cell off the map.
    pub fn of(&self, cell: Point) -> Option<PropertyId> {
        let index = self.size?.index_of(cell)?;
        PropertyId::new(*self.cells.get(index)?)
    }

    /// Whether whoever holds `key` may go to `cell` and use what is there:
    /// it is nobody's, or it is theirs.
    pub fn may_use(&self, cell: Point, key: Option<PropertyId>) -> bool {
        match self.of(cell) {
            None => true,
            owner => owner == key,
        }
    }

    /// How many properties there are.
    pub fn len(&self) -> usize {
        self.count as usize
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// The cells reached from `start` through `open` ones, or `None` if there
/// are more than [`LARGEST_PROPERTY`] of them — not a room but the outside.
/// Allocates: run once per locked door, with the world.
fn enclosed(size: Size, start: Point, open: &dyn Fn(Point) -> bool) -> Option<Vec<Point>> {
    let mut seen = vec![false; size.area()];
    let mut room = vec![start];
    seen[size.index_of(start)?] = true;
    let mut next = 0;
    while next < room.len() {
        for neighbour in room[next].cardinal_neighbours() {
            let Some(index) = size.index_of(neighbour) else {
                continue;
            };
            if !seen[index] && open(neighbour) {
                seen[index] = true;
                room.push(neighbour);
                if room.len() > LARGEST_PROPERTY {
                    return None;
                }
            }
        }
        next += 1;
    }
    // Lowest cell first, so the room is named the same whichever side of it
    // the flood began from.
    room.sort_unstable();
    Some(room)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::map::{FLOOR, WALL};
    use crate::sim::testing::prop_at;

    /// A walled room from `(2, 2)` to `(8, 7)` on a big open floor, a
    /// two-leaf house door in its bottom wall at x 4 and 5.
    pub(crate) fn house() -> Map {
        let mut map = Map::new(Size::new(60, 40), FLOOR);
        for y in 2..=7 {
            for x in 2..=8 {
                if x == 2 || x == 8 || y == 2 || y == 7 {
                    map.set_terrain(Point::new(x, y), WALL);
                }
            }
        }
        for x in [4, 5] {
            map.set_terrain(Point::new(x, 2), FLOOR);
            prop_at(&mut map, HOUSE_DOOR, Point::new(x, 2));
        }
        map
    }

    #[test]
    fn the_room_behind_a_house_door_is_a_property_and_the_street_is_nobody_s() {
        let properties = Properties::from_map(&house());
        assert_eq!(properties.len(), 1, "two leaves, one house");
        let home = properties.of(Point::new(5, 5)).expect("inside the house");
        for cell in [Point::new(3, 3), Point::new(7, 6), Point::new(4, 2), Point::new(5, 2)] {
            assert_eq!(properties.of(cell), Some(home), "{cell:?}");
        }
        for cell in [Point::new(4, 1), Point::new(30, 30), Point::new(2, 2), Point::new(-1, 0)] {
            assert_eq!(properties.of(cell), None, "{cell:?}");
        }
    }

    #[test]
    fn furniture_stands_in_the_property_it_is_in() {
        let mut map = house();
        prop_at(&mut map, "bed 1", Point::new(3, 6));
        let properties = Properties::from_map(&map);
        assert!(properties.of(Point::new(3, 6)).is_some(), "a bed's cell is the house's too");
    }

    #[test]
    fn only_the_owner_may_use_what_is_in_a_property_and_anybody_what_is_outside() {
        let properties = Properties::from_map(&house());
        let home = properties.of(Point::new(5, 5));
        assert!(properties.may_use(Point::new(5, 5), home));
        assert!(!properties.may_use(Point::new(5, 5), None));
        assert!(!properties.may_use(Point::new(5, 5), PropertyId::new(9)));
        assert!(properties.may_use(Point::new(20, 20), None));
        assert!(properties.may_use(Point::new(20, 20), home));
    }

    #[test]
    fn an_ordinary_door_locks_nothing() {
        let mut map = house();
        for object in map.objects(ObjectLayer::Props).to_vec() {
            map.remove_object(ObjectLayer::Props, &object);
        }
        for x in [4, 5] {
            prop_at(&mut map, super::super::door::DOOR, Point::new(x, 2));
        }
        assert!(Properties::from_map(&map).is_empty());
    }

    #[test]
    fn a_house_door_that_encloses_nothing_locks_nothing() {
        let mut map = Map::new(Size::new(60, 40), FLOOR);
        prop_at(&mut map, HOUSE_DOOR, Point::new(10, 10));
        assert!(Properties::from_map(&map).is_empty(), "open floor on every side");
    }

    #[test]
    fn two_houses_are_two_properties_numbered_the_same_way_every_time() {
        let mut map = house();
        for y in 2..=7 {
            for x in 20..=26 {
                if x == 20 || x == 26 || y == 2 || y == 7 {
                    map.set_terrain(Point::new(x, y), WALL);
                }
            }
        }
        map.set_terrain(Point::new(23, 7), FLOOR);
        prop_at(&mut map, HOUSE_DOOR, Point::new(23, 7));
        let properties = Properties::from_map(&map);
        assert_eq!(properties.len(), 2);
        let (first, second) = (properties.of(Point::new(5, 5)), properties.of(Point::new(23, 5)));
        assert_ne!(first, second);
        assert_eq!(first, PropertyId::new(1), "the lower door is numbered first");
        assert_eq!(properties, Properties::from_map(&map));
    }
}
