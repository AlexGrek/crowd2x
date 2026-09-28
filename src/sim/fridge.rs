//! A fridge's own state: [`Fridges`] — open or closed, and how cold it is
//! inside.
//!
//! Everything else about a fridge is timeless (`feature.rs`'s catalogue), but
//! a door and a temperature change *during* a tick, which is what the world
//! step ([`crate::sim::world_step`]) and [`crate::sim::Effect`] exist for: a
//! task asks, and this is applied to sequentially, in slot order, once the
//! whole crowd has reacted.

use crate::map::{Map, ObjectLayer, Point};
use crate::sim::clock::MINUTE;

/// The prop name a fridge is painted with — the only one [`Fridges::from_map`]
/// looks for.
pub const FRIDGE: &str = "fridge";

/// The room's temperature: what an open fridge warms toward.
pub const AMBIENT: f32 = 24.0;

/// As cold as a fridge's compressor can pull it: what a closed one cools
/// toward, and the floor an open one is clamped to as well.
pub const COLDEST: f32 = 4.0;

/// World seconds for an open fridge to close about two thirds of the gap to
/// [`AMBIENT`] — the time constant of the exponential approach, not a
/// duration anything waits out.
const WARMING: f32 = 5.0 * MINUTE;

/// The same, for a closed fridge cooling back toward [`COLDEST`]. Six times
/// slower than warming: a compressor works harder than a door lets heat in.
const COOLING: f32 = 30.0 * MINUTE;

/// Whether a fridge's door is open, and how cold it is inside.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FridgeState {
    open: bool,
    temperature: f32,
}

impl FridgeState {
    fn closed() -> FridgeState {
        FridgeState {
            open: false,
            temperature: COLDEST,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn temperature(&self) -> f32 {
        self.temperature
    }
}

/// Where the fridges are, and what state each is in.
///
/// Cells are found once, in [`GameState::new`](crate::sim::GameState::new),
/// exactly the way [`crate::sim::feature::Features`] is; unlike that index,
/// what is stored alongside them changes every tick a door is open. Kept
/// sorted so a lookup is a binary search and so nothing about it depends on
/// the order the fridges were painted in.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Fridges {
    cells: Vec<Point>,
    states: Vec<FridgeState>,
}

impl Fridges {
    pub fn from_map(map: &Map) -> Fridges {
        let mut cells: Vec<Point> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|object| object.kind.as_str() == FRIDGE)
            .map(|object| object.cell())
            .collect();
        cells.sort_unstable();
        let states = vec![FridgeState::closed(); cells.len()];
        Fridges { cells, states }
    }

    fn index(&self, cell: Point) -> Option<usize> {
        self.cells.binary_search(&cell).ok()
    }

    /// The state of the fridge in `cell`, or `None` when there is none.
    pub fn get(&self, cell: Point) -> Option<FridgeState> {
        self.index(cell).map(|i| self.states[i])
    }

    /// Whether the fridge in `cell` is open. `false` for a cell with no
    /// fridge in it.
    pub fn is_open(&self, cell: Point) -> bool {
        self.get(cell).is_some_and(|state| state.is_open())
    }

    /// How cold the fridge in `cell` is. [`COLDEST`] for a cell with no
    /// fridge — meaningless there, but never a fridge that reads colder than
    /// it can be.
    pub fn temperature(&self, cell: Point) -> f32 {
        self.get(cell).map_or(COLDEST, |state| state.temperature())
    }

    /// Every fridge, cell and state together.
    pub fn iter(&self) -> impl Iterator<Item = (Point, FridgeState)> + '_ {
        self.cells.iter().copied().zip(self.states.iter().copied())
    }

    /// Open or close the fridge in `cell`. `false`, and nothing changed, for
    /// a cell with no fridge in it.
    #[must_use = "a cell with no fridge in it refuses this"]
    pub fn set_open(&mut self, cell: Point, open: bool) -> bool {
        match self.index(cell) {
            Some(i) => {
                self.states[i].open = open;
                true
            }
            None => false,
        }
    }

    /// Time passing, in world seconds: every fridge's temperature moves
    /// toward what its door says — [`AMBIENT`] open, [`COLDEST`] closed —
    /// exponentially, at [`WARMING`] or [`COOLING`].
    ///
    /// Two [`f32::exp`] calls total, one per state, however many fridges
    /// there are: the factor is the same for every open fridge and the same
    /// for every closed one, so it is computed once and applied to each.
    pub fn advance(&mut self, game_dt: f32) {
        let open_factor = 1.0 - (-game_dt / WARMING).exp();
        let closed_factor = 1.0 - (-game_dt / COOLING).exp();
        for state in &mut self.states {
            let (target, factor) = if state.open {
                (AMBIENT, open_factor)
            } else {
                (COLDEST, closed_factor)
            };
            state.temperature = (state.temperature + (target - state.temperature) * factor).clamp(COLDEST, AMBIENT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Object, ObjectKind, Size, FLOOR, PIXELS_PER_CELL};

    fn prop(name: &str, cell: Point) -> Object {
        Object {
            at: Point::new(
                cell.x * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
                cell.y * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
            ),
            kind: ObjectKind::new(name),
        }
    }

    fn map_with_fridges(cells: &[Point]) -> Map {
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        for &cell in cells {
            map.add_object(ObjectLayer::Props, prop("fridge", cell));
        }
        map
    }

    #[test]
    fn a_fridge_starts_closed_and_as_cold_as_it_gets() {
        let cell = Point::new(3, 4);
        let fridges = Fridges::from_map(&map_with_fridges(&[cell]));
        let state = fridges.get(cell).expect("just painted one there");
        assert!(!state.is_open());
        assert_eq!(state.temperature(), COLDEST);
        assert!(!fridges.is_open(cell));
        assert_eq!(fridges.temperature(cell), COLDEST);
    }

    #[test]
    fn an_open_fridge_warms_on_every_tick_and_never_passes_room_temperature() {
        let cell = Point::new(3, 4);
        let mut fridges = Fridges::from_map(&map_with_fridges(&[cell]));
        assert!(fridges.set_open(cell, true));

        let mut last = COLDEST;
        for _ in 0..10_000 {
            fridges.advance(1.0);
            let now = fridges.temperature(cell);
            // Strictly rising until it is within a float32's precision of
            // the room, at which point a further tick's genuinely tiny
            // increment rounds away to nothing — that is convergence, not a
            // fridge that stopped warming.
            assert!(now > last || (AMBIENT - now).abs() < 1e-3, "should have kept rising: {now} after {last}");
            assert!(now <= AMBIENT, "{now} is warmer than the room");
            last = now;
        }
        assert!((last - AMBIENT).abs() < 0.01, "should have got close to room temperature: {last}");
    }

    #[test]
    fn a_closed_fridge_cools_back_down_and_never_below_the_coldest_it_gets() {
        let cell = Point::new(3, 4);
        let mut fridges = Fridges::from_map(&map_with_fridges(&[cell]));
        assert!(fridges.set_open(cell, true));
        for _ in 0..200 {
            fridges.advance(1.0);
        }
        let warm = fridges.temperature(cell);
        assert!(warm > COLDEST);

        assert!(fridges.set_open(cell, false));
        let mut last = warm;
        for _ in 0..20_000 {
            fridges.advance(1.0);
            let now = fridges.temperature(cell);
            assert!(now < last || (now - COLDEST).abs() < 1e-3, "should have kept falling: {now} after {last}");
            assert!(now >= COLDEST, "{now} is colder than a fridge gets");
            last = now;
        }
        assert!((last - COLDEST).abs() < 0.01, "should have got back to as cold as it gets: {last}");
    }

    /// The numbers CLAUDE.md quotes: about 10.6°C after two world minutes
    /// open (taking food out), about 7.6°C after one (pouring a drink).
    #[test]
    fn opening_a_fridge_for_a_meal_or_a_drink_warms_it_by_about_this_much() {
        let cell = Point::new(3, 4);
        let mut fridges = Fridges::from_map(&map_with_fridges(&[cell]));
        assert!(fridges.set_open(cell, true));
        fridges.advance(1.0 * MINUTE);
        assert!((fridges.temperature(cell) - 7.6).abs() < 0.1, "{}", fridges.temperature(cell));
        fridges.advance(1.0 * MINUTE);
        assert!((fridges.temperature(cell) - 10.6).abs() < 0.1, "{}", fridges.temperature(cell));
    }

    #[test]
    fn set_open_on_a_cell_with_no_fridge_in_it_refuses_and_changes_nothing() {
        let mut fridges = Fridges::from_map(&map_with_fridges(&[Point::new(1, 1)]));
        assert!(!fridges.set_open(Point::new(9, 9), true));
        assert_eq!(fridges.get(Point::new(9, 9)), None);
    }

    #[test]
    fn the_order_fridges_were_painted_in_does_not_change_the_lookup() {
        let cells = [Point::new(5, 5), Point::new(1, 1), Point::new(9, 2)];
        let forwards = Fridges::from_map(&map_with_fridges(&cells));
        let backwards = Fridges::from_map(&map_with_fridges(&[cells[2], cells[1], cells[0]]));
        assert_eq!(forwards, backwards);
        for cell in cells {
            assert!(forwards.get(cell).is_some());
        }
    }

    #[test]
    fn a_prop_that_is_not_a_fridge_is_not_one_here() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        map.add_object(ObjectLayer::Props, prop("crate", Point::new(2, 2)));
        let fridges = Fridges::from_map(&map);
        assert_eq!(fridges.get(Point::new(2, 2)), None);
    }
}
