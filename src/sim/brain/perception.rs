//! What a unit can see: [`Perception`].
//!
//! # What is seen
//!
//! **Whoever is standing in the cells in front of it**, out to [`FAR`] cells,
//! and nearest first. A unit looks the way it is walking ([`Heading`]), through
//! a cone [`HALF_ANGLE_DEGREES`] either side of that, plus the cells right
//! beside it; what is within [`NEAR`] cells is [`Range::Near`] and the rest
//! [`Range::Far`]. A wall is in the way of everything behind it; furniture,
//! and other people, are not.
//!
//! # What it costs, which is the point
//!
//! It is a **fixed number of cell lookups** per look, whatever the crowd: the
//! cells of a cone are worked out once per process, one table per heading
//! ([`cone`]), and a look walks the table asking the
//! [`Occupancy`](crate::sim::occupancy::Occupancy) grid — the spatial index
//! the simulation already keeps, one cell to a slot — who is standing in each.
//! About seventy reads, nearly all of them of empty cells; only a cell that has
//! somebody in it pays for a line of sight, at most [`FAR`] reads of the
//! terrain. Nothing scans the crowd, which is what killed the earlier
//! prototype. Away from the map's edge (nearly every look) an empty cell costs
//! one bit of [`Occupancy`](crate::sim::occupancy::Occupancy)'s occupied
//! bitmap and nothing else.
//!
//! And it does not happen every tick. A unit looks round on its own
//! [`Priority::Med`](crate::sim::background::Priority::Med) beat — every 7th
//! tick, at a point in that period its own schedule picks, so a crowd's
//! looking is spread evenly over the ticks rather than all on one. Attention
//! has limits; so does a frame.
//!
//! Which way a unit is facing is kept up every tick ([`Perception::turn`]),
//! since that is a couple of comparisons and a walker that turned a corner
//! between two looks should look down the new corridor.
//!
//! Who was seen is handed back from the look ([`Sightings`], on the stack) for
//! [`super::attention`] to decide whether any of it matters; all a unit keeps
//! of it is how many were near and how many far. **A look is mostly cache
//! misses** in a big crowd — whatever a unit keeps was last touched a look
//! ago — so what is not needed between looks is not kept.

use std::sync::LazyLock;

use crate::map::{Map, Point};
use crate::sim::entity::{Body, Think};
use crate::sim::uid::Uid;

/// Cells in front that count as close: within arm's reach of a few steps.
pub const NEAR: i32 = 3;

/// The furthest anything is seen, in cells.
pub const FAR: i32 = 8;

/// How far either side of straight ahead a unit sees, in degrees. A 120° field
/// — what a person takes in without turning their head.
pub const HALF_ANGLE_DEGREES: f32 = 60.0;

/// The most a unit takes in at one look, nearest first. Attention has limits:
/// a unit in the middle of a crowd notices the sixteen closest and not the
/// sixty behind them, and that is also what keeps a look a fixed size.
pub const SIGHTINGS: usize = 16;

/// One of the eight ways a unit can be facing. Y is up, as everywhere in `sim`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Heading {
    East,
    NorthEast,
    North,
    NorthWest,
    West,
    SouthWest,
    /// What a unit that has never moved faces: towards the viewer.
    #[default]
    South,
    SouthEast,
}

impl Heading {
    pub const ALL: [Heading; 8] = [
        Heading::East,
        Heading::NorthEast,
        Heading::North,
        Heading::NorthWest,
        Heading::West,
        Heading::SouthWest,
        Heading::South,
        Heading::SouthEast,
    ];

    /// One step this way, in cells.
    pub const fn step(self) -> (i32, i32) {
        match self {
            Heading::East => (1, 0),
            Heading::NorthEast => (1, 1),
            Heading::North => (0, 1),
            Heading::NorthWest => (-1, 1),
            Heading::West => (-1, 0),
            Heading::SouthWest => (-1, -1),
            Heading::South => (0, -1),
            Heading::SouthEast => (1, -1),
        }
    }

    /// The nearest of the eight to the direction `(dx, dy)`; `None` for no
    /// direction at all.
    ///
    /// No trigonometry: an axis wins when it is more than `tan(67.5°)` times
    /// the other, which is where the eight sectors meet.
    pub fn of(dx: f32, dy: f32) -> Option<Heading> {
        const TAN_67_5: f32 = 2.414_213_5;
        let (ax, ay) = (dx.abs(), dy.abs());
        if ax <= f32::EPSILON && ay <= f32::EPSILON {
            return None;
        }
        let sx = if ay > TAN_67_5 * ax { 0 } else { dx.signum() as i32 };
        let sy = if ax > TAN_67_5 * ay { 0 } else { dy.signum() as i32 };
        Heading::ALL.into_iter().find(|heading| heading.step() == (sx, sy))
    }

    pub const fn name(self) -> &'static str {
        match self {
            Heading::East => "east",
            Heading::NorthEast => "north-east",
            Heading::North => "north",
            Heading::NorthWest => "north-west",
            Heading::West => "west",
            Heading::SouthWest => "south-west",
            Heading::South => "south",
            Heading::SouthEast => "south-east",
        }
    }
}

/// How far away something was seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Range {
    /// Within [`NEAR`] cells.
    Near,
    /// Further, out to [`FAR`].
    Far,
}

/// Somebody seen, where, and how far off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sighting {
    pub uid: Uid,
    pub cell: Point,
    pub range: Range,
}

/// One test on a line of sight: the view is blocked here when **both** cells
/// are opaque. A cell the line passes through is a check of that cell twice;
/// a diagonal step of the line is a check of the two cells either side of the
/// corner it cuts, which is what stops a look slipping between two walls that
/// meet only at a corner.
///
/// Offsets from whoever is looking, in a byte each: nothing on a line is more
/// than [`FAR`] away.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Check {
    a: (i8, i8),
    b: (i8, i8),
}

/// The most checks a line can need: a cell and a corner for every step.
const MOST_CHECKS: usize = 2 * FAR as usize;

/// One cell of a cone, relative to whoever is looking — what [`Cone::cells`]
/// hands out, for anybody who wants the cone as a list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewCell {
    pub offset: Point,
    pub range: Range,
}

/// The checks a line of sight to one cell has to pass, nearest first.
///
/// Worked out here, once per process, rather than walked on every look: a
/// line depends only on the offset, never on where the viewer stands, so every
/// look in the game would walk the same few dozen lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Line {
    checks: [Check; MOST_CHECKS],
    len: u8,
}

impl Line {
    fn to(offset: Point) -> Line {
        let mut line = Line { checks: [Check { a: (0, 0), b: (0, 0) }; MOST_CHECKS], len: 0 };
        line_checks(offset, |check| {
            line.checks[line.len as usize] = check;
            line.len += 1;
            true
        });
        line
    }

    fn checks(&self) -> &[Check] {
        &self.checks[..self.len as usize]
    }
}

/// Every cell a unit facing one way can see, nearest first — ties broken by
/// position, so a look's order, and so the order things are noticed in, is the
/// same on every run.
///
/// **Laid out for the scan, not for reading.** What a look reads of every cell
/// is its offset, and most cells it reads are empty; so the offsets are an
/// array of their own, two bytes each and a few cache lines for the whole
/// cone, and a cell's line of sight — sixty-odd bytes — sits in a parallel
/// array that only an occupied cell touches. Kept together, a look walked
/// some seventy lines of table to find out that seventy cells were empty. And
/// since the cells are nearest first, the near ones are a prefix: a count, not
/// a field per cell.
pub struct Cone {
    offsets: Box<[(i8, i8)]>,
    /// How many of `offsets`, from the front, are [`Range::Near`].
    near: usize,
    /// Parallel to `offsets`.
    lines: Box<[Line]>,
}

impl Cone {
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    fn offset(&self, index: usize) -> Point {
        let (x, y) = self.offsets[index];
        Point::new(x as i32, y as i32)
    }

    fn range(&self, index: usize) -> Range {
        if index < self.near { Range::Near } else { Range::Far }
    }

    /// The cells, nearest first.
    pub fn cells(&self) -> impl Iterator<Item = ViewCell> + '_ {
        (0..self.len()).map(|i| ViewCell { offset: self.offset(i), range: self.range(i) })
    }

    /// What a line of sight to the `index`th cell has to pass.
    pub fn checks(&self, index: usize) -> &[Check] {
        self.lines[index].checks()
    }
}

/// The cone a unit facing `heading` sees through. Built once per process,
/// eight small tables; a look only reads them.
pub fn cone(heading: Heading) -> &'static Cone {
    static CONES: LazyLock<[Cone; 8]> = LazyLock::new(|| Heading::ALL.map(build_cone));
    &CONES[heading as usize]
}

fn build_cone(heading: Heading) -> Cone {
    let (hx, hy) = heading.step();
    let cos = HALF_ANGLE_DEGREES.to_radians().cos();
    let length = ((hx * hx + hy * hy) as f32).sqrt();
    let mut cells = Vec::new();
    for dy in -FAR..=FAR {
        for dx in -FAR..=FAR {
            let distance_sq = dx * dx + dy * dy;
            if distance_sq == 0 || distance_sq > FAR * FAR {
                continue;
            }
            let dot = (dx * hx + dy * hy) as f32;
            let distance = (distance_sq as f32).sqrt();
            let in_cone = dot >= cos * distance * length;
            // The cells touching it, out to either side: somebody at your
            // elbow is noticed even if they are not in front of you.
            let beside = dx.abs().max(dy.abs()) == 1 && dot >= 0.0;
            if in_cone || beside {
                cells.push(Point::new(dx, dy));
            }
        }
    }
    cells.sort_by_key(|cell| (cell.x.pow(2) + cell.y.pow(2), cell.y, cell.x));
    Cone {
        offsets: cells.iter().map(|cell| (cell.x as i8, cell.y as i8)).collect(),
        near: cells.iter().filter(|cell| cell.x.pow(2) + cell.y.pow(2) <= NEAR * NEAR).count(),
        lines: cells.iter().map(|&cell| Line::to(cell)).collect(),
    }
}

/// Hand `check` every test on the Bresenham line from the origin to `to`, in
/// order, until it returns `false`; returns whether it never did. The one
/// definition of a line of sight, for the tables and for [`in_sight`].
fn line_checks(to: Point, mut check: impl FnMut(Check) -> bool) -> bool {
    let (dx, dy) = (to.x.abs(), -to.y.abs());
    let (sx, sy) = (to.x.signum(), to.y.signum());
    let (mut x, mut y, mut error) = (0, 0, dx + dy);
    let byte = |x: i32, y: i32| (x as i8, y as i8);
    loop {
        let doubled = 2 * error;
        let (step_x, step_y) = (doubled >= dy, doubled <= dx);
        if step_x && step_y && !check(Check { a: byte(x + sx, y), b: byte(x, y + sy) }) {
            return false;
        }
        if step_x {
            error += dy;
            x += sx;
        }
        if step_y {
            error += dx;
            y += sy;
        }
        if (x, y) == (to.x, to.y) {
            return true;
        }
        if !check(Check { a: byte(x, y), b: byte(x, y) }) {
            return false;
        }
    }
}

/// Whether nothing blocks the view from `from` to `to` (no further than
/// [`FAR`] apart): every cell strictly between them, on a Bresenham line, is
/// terrain that could be walked on — and no diagonal step of the line squeezes
/// between two walls that meet only at a corner, which would be seeing through
/// a wall at its seam.
///
/// Terrain and not passability ([`Map::sight`]): a fridge or a bed stops a
/// walk but not a look over it. Off the map is opaque.
pub fn in_sight(map: &Map, from: Point, to: Point) -> bool {
    let clear = |(x, y): (i8, i8)| map.sight().is_passable(from.offset(x as i32, y as i32));
    line_checks(to - from, |check| clear(check.a) || clear(check.b))
}

/// Who one look took in, nearest first: a fixed array on the stack, handed
/// from [`Perception::look`] to whoever reacts to it and then gone.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sightings {
    seen: [Option<Sighting>; SIGHTINGS],
    len: u8,
}

impl Sightings {
    const NONE: Sightings = Sightings { seen: [None; SIGHTINGS], len: 0 };

    pub fn iter(&self) -> impl Iterator<Item = Sighting> + '_ {
        self.seen[..self.len as usize].iter().flatten().copied()
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push(&mut self, sighting: Sighting) {
        self.seen[self.len as usize] = Some(sighting);
        self.len += 1;
    }

    fn is_full(&self) -> bool {
        self.len as usize == SIGHTINGS
    }
}

/// What a unit can see: which way it faces, and how many it saw near and far
/// at its last look. Three bytes, inline: kept up every tick, and nothing in it
/// that a look's [`Sightings`] does not hand on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Perception {
    heading: Heading,
    near: u8,
    far: u8,
}

impl Perception {
    pub fn heading(&self) -> Heading {
        self.heading
    }

    /// Face the way the body is going, if it is going anywhere; otherwise keep
    /// facing the way it last went. Every tick, and cheap.
    pub fn turn(&mut self, direction: Option<(f32, f32)>) {
        if let Some(heading) = direction.and_then(|(dx, dy)| Heading::of(dx, dy)) {
            self.heading = heading;
        }
    }

    /// **Look round**: forget what was seen last time, and take in whoever is
    /// in the cone in front, nearest first, up to [`SIGHTINGS`] of them.
    ///
    /// Two scans with one body. Far enough from every edge that the whole cone
    /// is on the map — nearly every look, on any map worth a crowd — a cell is
    /// one row-major offset from the viewer's own index, read with no bounds
    /// checks of its own; near an edge, every cell is asked by its coordinates,
    /// and off the map answers nobody and opaque.
    pub fn look(&mut self, ctx: &Think<'_>, body: &Body) -> Sightings {
        let here = body.center_position();
        let me = body.uid();
        let occupancy = ctx.occupancy;
        let sight = ctx.map.sight();
        let size = occupancy.size();
        let interior = here.x >= FAR && here.y >= FAR && here.x + FAR < size.width && here.y + FAR < size.height;
        let sightings = if interior {
            let width = size.width as isize;
            let base = here.y as isize * width + here.x as isize;
            let at = move |x: i32, y: i32| (base + y as isize * width + x as isize) as usize;
            self.scan(
                me,
                here,
                |offset| occupancy.is_occupied_at(at(offset.x, offset.y)),
                |offset| occupancy.occupant_at(at(offset.x, offset.y)),
                |(x, y)| sight.is_passable_at(at(x as i32, y as i32)),
            )
        } else {
            self.scan(
                me,
                here,
                |offset| occupancy.is_occupied(here + offset),
                |offset| occupancy.occupant(here + offset),
                |(x, y)| sight.is_passable(here.offset(x as i32, y as i32)),
            )
        };
        let near = sightings.iter().filter(|seen| seen.range == Range::Near).count();
        self.near = near as u8;
        self.far = (sightings.len() - near) as u8;
        sightings
    }

    /// The look itself, over whichever way of reading a cell [`Perception::look`]
    /// picked: whether anybody is at an offset, who, and whether an offset can
    /// be seen through.
    ///
    /// In that order, cheapest first: a bit says a cell is empty, the line of
    /// sight is bits too, and *who* is there — a word out of an array the size
    /// of the map, and the read most likely to miss the cache — is only asked
    /// of somebody actually seen.
    #[inline(always)]
    fn scan(
        &self,
        me: Uid,
        here: Point,
        occupied: impl Fn(Point) -> bool,
        occupant: impl Fn(Point) -> Option<Uid>,
        clear: impl Fn((i8, i8)) -> bool,
    ) -> Sightings {
        let mut sightings = Sightings::NONE;
        let cone = cone(self.heading);
        for index in 0..cone.len() {
            let offset = cone.offset(index);
            if !occupied(offset) || !cone.checks(index).iter().all(|check| clear(check.a) || clear(check.b)) {
                continue;
            }
            let Some(uid) = occupant(offset) else {
                continue;
            };
            if uid == me {
                continue;
            }
            sightings.push(Sighting { uid, cell: here + offset, range: cone.range(index) });
            if sightings.is_full() {
                break;
            }
        }
        sightings
    }

    /// See nobody: what a look is while asleep.
    pub fn close_eyes(&mut self) {
        self.near = 0;
        self.far = 0;
    }

    /// How many it saw at its last look, near and far.
    pub fn saw(&self) -> (usize, usize) {
        (self.near as usize, self.far as usize)
    }

    /// For the brains menu. Allocates; never asked in a tick.
    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let (near, far) = self.saw();
        vec![
            ("facing", self.heading.name().to_string()),
            (
                "sees",
                if near + far == 0 { "nobody".to_string() } else { format!("{near} near, {far} far") },
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR, WALL};
    use crate::sim::testing::World;
    use crate::sim::uid::EntityType;

    fn uid(n: u64) -> Uid {
        Uid::new(EntityType::Human, n)
    }

    /// Who a unit at `cell` facing `heading` in `world` sees.
    fn look(world: &World, cell: Point, heading: Heading) -> Sightings {
        facing(heading).look(&world.ctx(), &Body::at_cell(uid(1), cell))
    }

    fn facing(heading: Heading) -> Perception {
        let (dx, dy) = heading.step();
        let mut perception = Perception::default();
        perception.turn(Some((dx as f32, dy as f32)));
        perception
    }

    fn put(world: &mut World, n: u64, cell: Point) {
        world.occupancy.claim(cell, uid(n)).unwrap();
    }

    #[test]
    fn a_direction_turns_into_the_nearest_of_eight_headings() {
        assert_eq!(Heading::of(1.0, 0.0), Some(Heading::East));
        assert_eq!(Heading::of(1.0, 0.3), Some(Heading::East));
        assert_eq!(Heading::of(1.0, 0.9), Some(Heading::NorthEast));
        assert_eq!(Heading::of(-0.2, 1.0), Some(Heading::North));
        assert_eq!(Heading::of(-1.0, -1.0), Some(Heading::SouthWest));
        assert_eq!(Heading::of(0.0, -3.0), Some(Heading::South));
        assert_eq!(Heading::of(0.0, 0.0), None);
        for heading in Heading::ALL {
            let (dx, dy) = heading.step();
            assert_eq!(Heading::of(dx as f32, dy as f32), Some(heading));
        }
    }

    #[test]
    fn standing_still_keeps_facing_the_way_it_last_went() {
        let mut perception = Perception::default();
        perception.turn(Some((-1.0, 0.0)));
        perception.turn(None);
        assert_eq!(perception.heading(), Heading::West);
    }

    #[test]
    fn a_cone_is_a_small_fixed_size_and_reaches_three_cells_near_and_eight_far() {
        for heading in Heading::ALL {
            let cells: Vec<ViewCell> = cone(heading).cells().collect();
            assert!((50..=90).contains(&cells.len()), "{heading:?}: {}", cells.len());
            let (hx, hy) = heading.step();
            let ahead = |n: i32| Point::new(hx * n, hy * n);
            let range_of = |offset: Point| cells.iter().find(|c| c.offset == offset).map(|c| c.range);
            if hx == 0 || hy == 0 {
                assert_eq!(range_of(ahead(3)), Some(Range::Near), "{heading:?}");
                assert_eq!(range_of(ahead(4)), Some(Range::Far), "{heading:?}");
                assert_eq!(range_of(ahead(8)), Some(Range::Far), "{heading:?}");
                assert_eq!(range_of(ahead(9)), None, "{heading:?}");
            }
            assert_eq!(range_of(Point::new(-hx, -hy)), None, "{heading:?}: nothing behind");
            assert_eq!(range_of(ahead(-3)), None, "{heading:?}: nothing behind");
        }
    }

    #[test]
    fn a_cone_is_nearest_first() {
        let distances: Vec<i32> = cone(Heading::North)
            .cells()
            .map(|c| c.offset.x.pow(2) + c.offset.y.pow(2))
            .collect();
        assert!(distances.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn somebody_in_front_is_seen_near_or_far_and_somebody_behind_is_not() {
        let mut world = World::new(Map::new(Size::new(20, 20), FLOOR));
        let me = Point::new(10, 5);
        put(&mut world, 2, Point::new(10, 7));
        put(&mut world, 3, Point::new(11, 12));
        put(&mut world, 4, Point::new(10, 3));
        let seen: Vec<Sighting> = look(&world, me, Heading::North).iter().collect();
        assert_eq!(
            seen,
            [
                Sighting { uid: uid(2), cell: Point::new(10, 7), range: Range::Near },
                Sighting { uid: uid(3), cell: Point::new(11, 12), range: Range::Far },
            ]
        );
    }

    #[test]
    fn a_unit_does_not_see_itself_or_anybody_past_its_range() {
        let mut world = World::new(Map::new(Size::new(20, 20), FLOOR));
        let me = Point::new(10, 5);
        put(&mut world, 1, me);
        put(&mut world, 2, Point::new(10, 14));
        assert_eq!(look(&world, me, Heading::North).iter().count(), 0);
    }

    #[test]
    fn a_wall_hides_whoever_is_behind_it_and_furniture_does_not() {
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        for x in 0..20 {
            map.set_terrain(Point::new(x, 8), WALL);
        }
        crate::sim::testing::prop_at(&mut map, "fridge", Point::new(10, 6));
        let mut world = World::new(map);
        let me = Point::new(10, 5);
        put(&mut world, 2, Point::new(10, 7));
        put(&mut world, 3, Point::new(10, 10));
        let seen: Vec<Uid> = look(&world, me, Heading::North).iter().map(|s| s.uid).collect();
        assert_eq!(seen, [uid(2)], "seen over the fridge, not through the wall");
    }

    #[test]
    fn two_walls_meeting_at_a_corner_cannot_be_seen_between() {
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        // A staircase of wall running north-west to south-east between the
        // unit at (5, 5) and whoever is at (7, 7): each step touches the next
        // only at a corner.
        for i in 0..8 {
            map.set_terrain(Point::new(2 + i, 9 - i), WALL);
        }
        let mut world = World::new(map);
        let me = Point::new(5, 5);
        put(&mut world, 2, Point::new(7, 7));
        put(&mut world, 3, Point::new(6, 6));
        assert!(!in_sight(&world.map, me, Point::new(7, 7)));
        let cone = cone(Heading::NorthEast);
        assert!((0..cone.len()).all(|i| cone.checks(i).len() <= MOST_CHECKS));
        assert!(!in_sight(&world.map, me, Point::new(6, 6)), "even right beside it");
        assert_eq!(look(&world, me, Heading::NorthEast).iter().count(), 0);
        // With a gap in it there is a view.
        world.map.set_terrain(Point::new(6, 5), FLOOR);
        assert!(in_sight(&world.map, me, Point::new(6, 6)));
    }

    /// The two scans must agree: one unit at the edge of the map and one in
    /// the middle, with the same people around each, see the same people.
    #[test]
    fn a_look_from_the_middle_and_a_look_from_the_edge_see_alike() {
        let mut map = Map::new(Size::new(40, 40), FLOOR);
        for (x, y) in [(22, 23), (2, 3), (24, 22), (4, 2), (21, 24), (1, 4)] {
            map.set_terrain(Point::new(x, y), WALL);
        }
        let mut world = World::new(map);
        let (middle, edge) = (Point::new(20, 20), Point::new(0, 0));
        let mut n = 2;
        for (dx, dy) in [(1, 1), (3, 4), (5, 5), (2, 6), (6, 2), (0, 3), (4, 0), (7, 3)] {
            put(&mut world, n, middle.offset(dx, dy));
            put(&mut world, n + 1, edge.offset(dx, dy));
            n += 2;
        }
        let offsets = |from: Point| -> Vec<(Point, Range)> {
            look(&world, from, Heading::NorthEast).iter().map(|s| (s.cell - from, s.range)).collect()
        };
        assert!(offsets(middle).len() >= 4, "{:?}", offsets(middle));
        assert!(offsets(middle).len() < 8, "and the walls hid somebody: {:?}", offsets(middle));
        assert_eq!(offsets(middle), offsets(edge));
    }

    #[test]
    fn in_a_crowd_only_the_nearest_are_taken_in() {
        let mut world = World::new(Map::new(Size::new(30, 30), FLOOR));
        let me = Point::new(15, 5);
        let mut n = 2;
        for cell in cone(Heading::North).cells() {
            put(&mut world, n, me + cell.offset);
            n += 1;
        }
        let seen: Vec<Sighting> = look(&world, me, Heading::North).iter().collect();
        assert_eq!(seen.len(), SIGHTINGS);
        let nearest: Vec<Point> = cone(Heading::North).cells().take(SIGHTINGS).map(|c| me + c.offset).collect();
        assert_eq!(seen.iter().map(|s| s.cell).collect::<Vec<_>>(), nearest);
    }

    #[test]
    fn a_look_forgets_what_the_last_one_saw() {
        let mut world = World::new(Map::new(Size::new(20, 20), FLOOR));
        let me = Point::new(10, 5);
        put(&mut world, 2, Point::new(10, 7));
        let mut perception = facing(Heading::North);
        assert_eq!(perception.look(&world.ctx(), &Body::at_cell(uid(1), me)).len(), 1);
        assert_eq!(perception.saw(), (1, 0));
        world.occupancy.clear();
        assert!(perception.look(&world.ctx(), &Body::at_cell(uid(1), me)).is_empty());
        assert_eq!(perception.saw(), (0, 0));
    }
}
