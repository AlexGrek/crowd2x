//! Doors: [`Doors`], each one's leaf open, shut or on its way, and who may
//! open it.
//!
//! A door is a prop in a doorway — [`DOOR`] for anybody, [`HOUSE_DOOR`] for
//! the owners of the property behind it ([`crate::sim::property`]). The map
//! lets a route through one (`map::PROPS` says a door does not block): **what
//! stops a body is the leaf, and the leaf is state**, which is the fridge's
//! shape again — it changes during play, so it lives here rather than on the
//! map, and a unit asks for it through an [`Effect`](crate::sim::Effect)
//! rather than writing it.
//!
//! # Opening takes time, and whoever is waiting waits
//!
//! A walker whose next cell is a door that is not fully open stands still
//! (`Walker::think`) and asks for it to open — [`Effect::Door`], every tick
//! its next cell is a door, open or not, which is also what holds it open
//! while a crowd is going through. The world step settles the asks in slot
//! order ([`Doors::want`]) and then moves every leaf on ([`Doors::advance`]):
//! a wanted door opens over [`OPENING`], stays open for [`HELD_OPEN`] after
//! the last ask, and then shuts over [`CLOSING`] — **but never on somebody
//! standing in the doorway**. Only a fully open door is walked into; one
//! even slightly shut is a wall to the move step.
//!
//! These are watched durations, not world ones: a leaf swinging is something
//! seen happening, measured on `Think::dt` like a walk, and written in world
//! time converted at the point it is defined ([`watched`]).
//!
//! # Locked
//!
//! A house door opens only for a unit whose [`Body::home`] is the property it
//! locks. Its routes are planned round every door it holds no key to
//! ([`Doors::lets_through`], asked by both stages of a walk), so it never
//! waits at one, and the world step refuses an ask from anybody else anyway.
//!
//! [`Effect::Door`]: crate::sim::Effect::Door
//! [`Body::home`]: crate::sim::Body::home

use crate::map::{Map, ObjectLayer, Point, Size};
use crate::sim::clock::{watched, MINUTE};

use super::occupancy::Occupancy;
use super::property::{Properties, PropertyId};

/// A door anybody may open: a shop's, the bank's, a restroom's.
pub const DOOR: &str = "door";

/// A door only the owners of the property behind it may open: a house's.
pub const HOUSE_DOOR: &str = "house door";

/// Every prop that is a door.
pub const DOORS: [&str; 2] = [DOOR, HOUSE_DOOR];

/// Watched seconds for a shut door to open all the way: a world minute.
pub const OPENING: f32 = watched(MINUTE);

/// ...and for an open one to shut.
pub const CLOSING: f32 = watched(MINUTE);

/// Watched seconds a door stays open after the last unit asked for it —
/// long enough for whoever asked to cross the doorway before it starts to
/// shut.
pub const HELD_OPEN: f32 = watched(2.0 * MINUTE);

/// Whether a prop called `name` is a door.
pub fn is_door(name: &str) -> bool {
    DOORS.contains(&name)
}

/// One door's leaf.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DoorState {
    /// 0 shut, 1 open, and on its way in between.
    openness: f32,
    /// Watched seconds left before it starts to shut, counting down from
    /// [`HELD_OPEN`] at the last ask.
    held: f32,
    /// The property only whose owners may open it; `None` for anybody.
    lock: Option<PropertyId>,
}

impl DoorState {
    /// How far open, 0 shut to 1 open — what the leaf is drawn from.
    pub fn openness(&self) -> f32 {
        self.openness
    }

    /// Open all the way: the one state a body may walk into.
    pub fn is_open(&self) -> bool {
        self.openness >= 1.0
    }

    pub fn lock(&self) -> Option<PropertyId> {
        self.lock
    }

    /// Whether whoever holds `key` may open it.
    pub fn opens_for(&self, key: Option<PropertyId>) -> bool {
        self.lock.is_none() || self.lock == key
    }
}

/// Where the doors are, and what each one's leaf is doing.
///
/// Built once with the world, like [`super::fridge::Fridges`], and kept
/// sorted so a lookup is a binary search and nothing depends on the order
/// the doors were painted in. Beside it a bit per map cell for the locked
/// ones, since a route asks about every cell it expands and almost none of
/// them is a door.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Doors {
    cells: Vec<Point>,
    states: Vec<DoorState>,
    /// `None` only for the empty default, which has no doors.
    size: Option<Size>,
    locked: Vec<u64>,
}

impl Doors {
    /// Every door on `map`, shut, each house door locked to the property
    /// `properties` says is behind it.
    pub fn from_map(map: &Map, properties: &Properties) -> Doors {
        let mut doors: Vec<(Point, Option<PropertyId>)> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|object| is_door(object.kind.as_str()))
            .map(|object| {
                let cell = object.cell();
                let lock = if object.kind.as_str() == HOUSE_DOOR { properties.of(cell) } else { None };
                (cell, lock)
            })
            .collect();
        doors.sort_unstable_by_key(|&(cell, _)| cell);
        doors.dedup_by_key(|&mut (cell, _)| cell);

        let size = map.size();
        let mut locked = vec![0u64; size.area().div_ceil(64)];
        for &(cell, lock) in &doors {
            if lock.is_some()
                && let Some(index) = size.index_of(cell)
            {
                locked[index / 64] |= 1 << (index % 64);
            }
        }
        Doors {
            cells: doors.iter().map(|&(cell, _)| cell).collect(),
            states: doors
                .iter()
                .map(|&(_, lock)| DoorState {
                    openness: 0.0,
                    held: 0.0,
                    lock,
                })
                .collect(),
            size: Some(size),
            locked,
        }
    }

    fn index(&self, cell: Point) -> Option<usize> {
        self.cells.binary_search(&cell).ok()
    }

    /// The door in `cell`, or `None` when there is none.
    pub fn get(&self, cell: Point) -> Option<DoorState> {
        self.index(cell).map(|i| self.states[i])
    }

    /// Whether there is a door in `cell`.
    pub fn is_door(&self, cell: Point) -> bool {
        self.index(cell).is_some()
    }

    /// Whether `cell` has a door in it that is not fully open — a wall, for
    /// now, to anybody walking. `false` for a cell with no door.
    pub fn is_shut(&self, cell: Point) -> bool {
        self.get(cell).is_some_and(|door| !door.is_open())
    }

    /// How far open the door in `cell` is; `0` for a cell with no door.
    pub fn openness(&self, cell: Point) -> f32 {
        self.get(cell).map_or(0.0, |door| door.openness)
    }

    /// Whether whoever holds `key` may plan a route through `cell`: there is
    /// no locked door in it, or they hold its key. A bit for every cell that
    /// is not a locked door, which is what a route asks about.
    pub fn lets_through(&self, cell: Point, key: Option<PropertyId>) -> bool {
        let Some(index) = self.size.and_then(|size| size.index_of(cell)) else {
            return true;
        };
        if self.locked.get(index / 64).is_none_or(|word| word & (1 << (index % 64)) == 0) {
            return true;
        }
        self.get(cell).is_none_or(|door| door.opens_for(key))
    }

    /// Every door, cell and state together.
    pub fn iter(&self) -> impl Iterator<Item = (Point, DoorState)> + '_ {
        self.cells.iter().copied().zip(self.states.iter().copied())
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Somebody holding `key` wants to go through the door in `cell`: start
    /// it opening, or keep it open, for [`HELD_OPEN`] from now. `false`, and
    /// nothing changed, for a cell with no door or a door that does not open
    /// for that key.
    #[must_use = "a door that is not there, or is locked to this key, refuses"]
    pub fn want(&mut self, cell: Point, key: Option<PropertyId>) -> bool {
        match self.index(cell) {
            Some(i) if self.states[i].opens_for(key) => {
                self.states[i].held = HELD_OPEN;
                true
            }
            _ => false,
        }
    }

    /// Time passing, in **watched** seconds: a wanted door opens, one nobody
    /// has wanted for [`HELD_OPEN`] shuts — unless somebody is standing in
    /// it. No allocation; one pass over the doors.
    pub fn advance(&mut self, dt: f32, occupancy: &Occupancy) {
        for (cell, state) in self.cells.iter().zip(&mut self.states) {
            if state.held > 0.0 {
                state.held = (state.held - dt).max(0.0);
                state.openness = (state.openness + dt / OPENING).min(1.0);
            } else if state.openness > 0.0 && !occupancy.is_occupied(*cell) {
                state.openness = (state.openness - dt / CLOSING).max(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR};
    use crate::sim::testing::prop_at;
    use crate::sim::uid::{EntityType, Uid};

    const DT: f32 = 1.0 / 64.0;

    fn doors_on(map: &Map) -> Doors {
        Doors::from_map(map, &Properties::from_map(map))
    }

    fn one_door() -> (Doors, Occupancy, Point) {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        let cell = Point::new(4, 4);
        prop_at(&mut map, DOOR, cell);
        (doors_on(&map), Occupancy::new(map.size()), cell)
    }

    fn run(doors: &mut Doors, occupancy: &Occupancy, seconds: f32) {
        for _ in 0..(seconds / DT).ceil() as usize {
            doors.advance(DT, occupancy);
        }
    }

    #[test]
    fn a_door_starts_shut_and_is_a_wall_until_it_is_all_the_way_open() {
        let (mut doors, occupancy, cell) = one_door();
        assert!(doors.is_shut(cell));
        assert!(doors.want(cell, None));
        run(&mut doors, &occupancy, OPENING / 2.0);
        let half = doors.openness(cell);
        assert!(half > 0.3 && half < 0.7, "{half}");
        assert!(doors.is_shut(cell), "half open is still shut to a body");
        run(&mut doors, &occupancy, OPENING);
        assert!(!doors.is_shut(cell));
        assert_eq!(doors.openness(cell), 1.0);
    }

    #[test]
    fn an_open_door_nobody_wants_shuts_again_after_a_while() {
        let (mut doors, occupancy, cell) = one_door();
        assert!(doors.want(cell, None));
        run(&mut doors, &occupancy, OPENING + DT);
        assert!(!doors.is_shut(cell));
        run(&mut doors, &occupancy, HELD_OPEN - OPENING - 2.0 * DT);
        assert!(!doors.is_shut(cell), "still held open");
        run(&mut doors, &occupancy, CLOSING + 4.0 * DT);
        assert_eq!(doors.openness(cell), 0.0);
    }

    #[test]
    fn a_door_never_shuts_on_somebody_standing_in_it() {
        let (mut doors, mut occupancy, cell) = one_door();
        assert!(doors.want(cell, None));
        run(&mut doors, &occupancy, OPENING + DT);
        occupancy.claim(cell, Uid::new(EntityType::Human, 1)).unwrap();
        run(&mut doors, &occupancy, HELD_OPEN + CLOSING * 2.0);
        assert!(!doors.is_shut(cell));
        occupancy.release(cell, Uid::new(EntityType::Human, 1));
        run(&mut doors, &occupancy, CLOSING + DT);
        assert!(doors.is_shut(cell));
    }

    #[test]
    fn asking_again_while_it_shuts_opens_it_again() {
        let (mut doors, occupancy, cell) = one_door();
        assert!(doors.want(cell, None));
        run(&mut doors, &occupancy, HELD_OPEN + CLOSING / 2.0);
        let closing = doors.openness(cell);
        assert!(closing < 1.0 && closing > 0.0);
        assert!(doors.want(cell, None));
        doors.advance(DT, &occupancy);
        assert!(doors.openness(cell) > closing);
    }

    #[test]
    fn a_house_door_opens_for_its_owners_and_nobody_else() {
        let map = super::super::property::tests::house();
        let properties = Properties::from_map(&map);
        let mut doors = Doors::from_map(&map, &properties);
        let door = Point::new(4, 2);
        let home = properties.of(door);
        assert!(home.is_some());
        assert_eq!(doors.get(door).unwrap().lock(), home);

        assert!(!doors.lets_through(door, None));
        assert!(!doors.lets_through(door, PropertyId::new(7)));
        assert!(doors.lets_through(door, home));
        assert!(doors.lets_through(Point::new(4, 1), None), "not a door");

        assert!(!doors.want(door, None), "a stranger is refused");
        assert!(doors.want(door, home));
    }

    #[test]
    fn a_cell_with_no_door_is_never_shut_and_refuses_to_open() {
        let (mut doors, _, _) = one_door();
        let elsewhere = Point::new(1, 1);
        assert!(!doors.is_shut(elsewhere));
        assert!(!doors.is_door(elsewhere));
        assert!(!doors.want(elsewhere, None));
        assert!(doors.lets_through(elsewhere, None));
    }
}
