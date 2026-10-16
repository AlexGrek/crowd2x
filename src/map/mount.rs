//! Things fixed to a wall: where one may go, which wall it is on, and where
//! on that wall.
//!
//! A [`Prop::wall_mounted`](super::Prop::wall_mounted) prop — a light switch —
//! is in a floor cell like any prop ([`Object::cell`]'s rule), and fixed to
//! one of the walls beside that cell. It goes only where there is such a wall,
//! on floor nothing else stands in ([`can_mount`]): one cell, and still a cell
//! anybody can walk through.
//!
//! **Which wall is the one nearest its position** ([`wall_of`]), so whoever
//! places one in a corner picks the wall by pointing at it. Placing keeps the
//! position on that wall's edge of the cell ([`mounted_at`]), so the map file
//! says which wall it is and nothing has to remember where the pointer was.
//! A position nearer the middle than any edge — a hand-written file — goes to
//! the walls in [`Side::ALL`] order: the far one first.
//!
//! A wall here is terrain neither a body nor sight crosses: a wall tile or a
//! block. Not a window, which is glass, and not the void — an unbuilt cell or
//! the edge of the map is not something to screw a switch to.

use super::{Map, ObjectLayer, Point, PIXELS_PER_CELL, VOID};

/// Which side of its cell a wall is on. Y increases upward, so `Up` is the
/// wall above: the far wall of a room, seen from above.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Up,
    Down,
    Left,
    Right,
}

impl Side {
    /// Every side, in the order a tie between two walls is broken: the far
    /// wall, the two side walls, the near wall.
    pub const ALL: [Side; 4] = [Side::Up, Side::Left, Side::Right, Side::Down];

    /// The step from a cell to its neighbour on this side.
    pub const fn step(self) -> Point {
        match self {
            Side::Up => Point::new(0, 1),
            Side::Down => Point::new(0, -1),
            Side::Left => Point::new(-1, 0),
            Side::Right => Point::new(1, 0),
        }
    }

    /// Quarter turns anticlockwise that bring art drawn for a wall above —
    /// the way every wall-mounted prop is drawn — onto this wall.
    pub const fn quarter_turns(self) -> u8 {
        match self {
            Side::Up => 0,
            Side::Left => 1,
            Side::Down => 2,
            Side::Right => 3,
        }
    }
}

/// Whether `cell` is a wall something can be fixed to.
pub fn is_wall(map: &Map, cell: Point) -> bool {
    map.terrain(cell)
        .is_some_and(|tile| tile != VOID && !tile.is_passable() && !tile.is_see_through())
}

/// Whether a wall-mounted prop can go in `cell`: floor with nothing standing
/// in it — no furniture, no door, no other switch — and a wall beside it.
pub fn can_mount(map: &Map, cell: Point) -> bool {
    map.terrain(cell).is_some_and(|tile| tile.is_passable())
        && !map.objects(ObjectLayer::Props).iter().any(|prop| prop.cell() == cell)
        && Side::ALL.into_iter().any(|side| is_wall(map, cell + side.step()))
}

/// The wall a wall-mounted prop at `at` (whole pixels) is fixed to: of the
/// walls beside its cell, the one whose edge `at` is nearest. `None` with no
/// wall beside it.
pub fn wall_of(map: &Map, at: Point) -> Option<Side> {
    let cell = cell_of(at);
    // To the cell's edges as lines, 0 and a cell along, so its centre is as
    // near one as another.
    let (dx, dy) = (at.x - cell.x * PIXELS_PER_CELL, at.y - cell.y * PIXELS_PER_CELL);
    let distance = |side: Side| match side {
        Side::Up => PIXELS_PER_CELL - dy,
        Side::Down => dy,
        Side::Left => dx,
        Side::Right => PIXELS_PER_CELL - dx,
    };
    // The first of equals, so a tie goes the way `Side::ALL` reads.
    Side::ALL
        .into_iter()
        .filter(|&side| is_wall(map, cell + side.step()))
        .min_by_key(|&side| distance(side))
}

/// Where a prop fixed to the `side` wall of `cell` is kept: the middle of
/// that edge, on the cell's last row or column of pixels — still in the cell,
/// and nearer that wall than any other.
pub fn mounted_at(cell: Point, side: Side) -> Point {
    let (x, y) = (cell.x * PIXELS_PER_CELL, cell.y * PIXELS_PER_CELL);
    let (middle, last) = (PIXELS_PER_CELL / 2, PIXELS_PER_CELL - 1);
    match side {
        Side::Up => Point::new(x + middle, y + last),
        Side::Down => Point::new(x + middle, y),
        Side::Left => Point::new(x, y + middle),
        Side::Right => Point::new(x + last, y + middle),
    }
}

/// Where a wall-mounted prop put down at `at` goes: onto the wall of its cell
/// nearest `at`, or nowhere when the cell is no place for one
/// ([`can_mount`]).
pub fn mount(map: &Map, at: Point) -> Option<Point> {
    let cell = cell_of(at);
    if !can_mount(map, cell) {
        return None;
    }
    wall_of(map, at).map(|side| mounted_at(cell, side))
}

/// [`Object::cell`](super::Object::cell)'s rule, for a bare position.
fn cell_of(at: Point) -> Point {
    Point::new(at.x.div_euclid(PIXELS_PER_CELL), at.y.div_euclid(PIXELS_PER_CELL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Object, ObjectKind, Size, TerrainId, FLOOR, WALL};

    /// A walled 7x6 room: floor from (1, 1) to (5, 4).
    fn room() -> Map {
        let mut map = Map::new(Size::new(7, 6), FLOOR);
        for cell in map.size().points().collect::<Vec<_>>() {
            if cell.x == 0 || cell.y == 0 || cell.x == 6 || cell.y == 5 {
                map.set_terrain(cell, WALL);
            }
        }
        map
    }

    fn centre(cell: Point) -> Point {
        Point::new(cell.x * PIXELS_PER_CELL + PIXELS_PER_CELL / 2, cell.y * PIXELS_PER_CELL + PIXELS_PER_CELL / 2)
    }

    fn put(map: &mut Map, name: &str, cell: Point) {
        map.add_object(ObjectLayer::Props, Object { at: centre(cell), kind: ObjectKind::new(name) });
    }

    #[test]
    fn it_goes_only_on_floor_beside_a_wall() {
        let map = room();
        assert!(can_mount(&map, Point::new(3, 4)), "under the top wall");
        assert!(can_mount(&map, Point::new(1, 2)), "beside the left one");
        assert!(!can_mount(&map, Point::new(3, 2)), "the middle of the room");
        assert!(!can_mount(&map, Point::new(3, 5)), "in the wall itself");
        assert!(!can_mount(&map, Point::new(-1, 2)), "off the map");
        assert_eq!(mount(&map, centre(Point::new(3, 2))), None);
    }

    #[test]
    fn a_window_and_the_void_are_not_walls() {
        let mut map = room();
        let window = TerrainId::from_name("window").unwrap();
        map.set_terrain(Point::new(3, 5), window);
        map.set_terrain(Point::new(2, 5), VOID);
        map.set_terrain(Point::new(4, 5), VOID);
        assert!(!can_mount(&map, Point::new(3, 4)), "under a window");
        assert!(!can_mount(&map, Point::new(2, 4)), "under nothing");
        assert!(is_wall(&map, Point::new(5, 5)));
        assert!(!is_wall(&map, Point::new(3, 7)), "off the map");
    }

    #[test]
    fn it_shares_its_cell_with_nothing_that_stands_there() {
        let mut map = room();
        put(&mut map, "fridge", Point::new(1, 4));
        put(&mut map, "door", Point::new(2, 4));
        put(&mut map, "light switch", Point::new(3, 4));
        for x in 1..=3 {
            assert!(!can_mount(&map, Point::new(x, 4)), "x {x}");
        }
        assert!(can_mount(&map, Point::new(4, 4)));
        // A lamp hangs from the ceiling, above a switch's head.
        map.add_object(ObjectLayer::Lamps, Object { at: centre(Point::new(4, 4)), kind: ObjectKind::new("ceiling lamp") });
        assert!(can_mount(&map, Point::new(4, 4)));
    }

    #[test]
    fn it_is_put_on_the_wall_nearest_where_it_was_put_down() {
        let map = room();
        // The top-left corner: a wall above and a wall to the left.
        let corner = Point::new(1, 4);
        let base = Point::new(PIXELS_PER_CELL, 4 * PIXELS_PER_CELL);
        assert_eq!(wall_of(&map, base + Point::new(4, 30)), Some(Side::Left));
        assert_eq!(wall_of(&map, base + Point::new(30, 44)), Some(Side::Up));
        assert_eq!(wall_of(&map, centre(corner)), Some(Side::Up), "a tie goes to the far wall");
        // ...and with no far wall, to the left one before the right.
        let mut pocket = Map::new(Size::new(3, 3), FLOOR);
        pocket.set_terrain(Point::new(0, 1), WALL);
        pocket.set_terrain(Point::new(2, 1), WALL);
        assert_eq!(wall_of(&pocket, centre(Point::new(1, 1))), Some(Side::Left));
        assert_eq!(mount(&map, base + Point::new(4, 30)), Some(mounted_at(corner, Side::Left)));
        // A wall that is not there is never chosen, however near.
        assert_eq!(wall_of(&map, Point::new(3 * PIXELS_PER_CELL + 24, 4 * PIXELS_PER_CELL + 1)), Some(Side::Up));
        assert_eq!(wall_of(&map, centre(Point::new(3, 2))), None);
    }

    /// The position a prop is kept at says its wall, and its cell, again.
    #[test]
    fn where_it_is_kept_is_in_its_cell_and_nearest_its_wall() {
        // One floor cell walled in on every side.
        let mut map = Map::new(Size::new(3, 3), WALL);
        let cell = Point::new(1, 1);
        map.set_terrain(cell, FLOOR);
        for side in Side::ALL {
            let at = mounted_at(cell, side);
            assert_eq!(cell_of(at), cell, "{side:?}");
            assert_eq!(wall_of(&map, at), Some(side));
            assert_eq!(mount(&map, at), Some(at), "{side:?}: placing it again leaves it where it is");
        }
    }

    #[test]
    fn art_for_the_wall_above_turns_to_fit_each_other_wall() {
        let turns: Vec<u8> = Side::ALL.map(Side::quarter_turns).to_vec();
        assert_eq!(turns, [0, 1, 3, 2]);
        // A quarter turn anticlockwise takes the top of the art to the left.
        let up = Side::Up.step();
        let turned = Point::new(-up.y, up.x);
        assert_eq!(turned, Side::Left.step());
    }
}
