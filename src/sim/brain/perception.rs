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
//! prototype.
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
//! What was seen is kept until the next look ([`Perception::seen`]), and
//! [`super::attention`] is what decides whether any of it matters.

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

/// One cell of a cone, relative to whoever is looking.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewCell {
    pub offset: Point,
    pub range: Range,
}

/// Every cell a unit facing `heading` can see, nearest first — ties broken by
/// position, so a look's order, and so the order things are noticed in, is the
/// same on every run.
///
/// Built once per process, eight small tables; a look only reads them.
pub fn cone(heading: Heading) -> &'static [ViewCell] {
    static CONES: LazyLock<[Box<[ViewCell]>; 8]> =
        LazyLock::new(|| Heading::ALL.map(|heading| build_cone(heading).into_boxed_slice()));
    &CONES[heading as usize]
}

fn build_cone(heading: Heading) -> Vec<ViewCell> {
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
                let range = if distance_sq <= NEAR * NEAR { Range::Near } else { Range::Far };
                cells.push(ViewCell { offset: Point::new(dx, dy), range });
            }
        }
    }
    cells.sort_by_key(|cell| (cell.offset.x.pow(2) + cell.offset.y.pow(2), cell.offset.y, cell.offset.x));
    cells
}

/// Whether nothing blocks the view from `from` to `to`: every cell strictly
/// between them, on a Bresenham line, is terrain that could be walked on —
/// and no diagonal step of the line squeezes between two walls that meet only
/// at a corner, which would be seeing through a wall at its seam.
///
/// Terrain and not passability: a fridge or a bed stops a walk but not a look
/// over it. Off the map is opaque.
pub fn in_sight(map: &Map, from: Point, to: Point) -> bool {
    let clear = |cell: Point| map.terrain(cell).is_some_and(|tile| tile.is_passable());
    let (dx, dy) = ((to.x - from.x).abs(), -(to.y - from.y).abs());
    let (sx, sy) = ((to.x - from.x).signum(), (to.y - from.y).signum());
    let (mut x, mut y, mut error) = (from.x, from.y, dx + dy);
    loop {
        let doubled = 2 * error;
        let (step_x, step_y) = (doubled >= dy, doubled <= dx);
        if step_x && step_y && !clear(Point::new(x + sx, y)) && !clear(Point::new(x, y + sy)) {
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
        let cell = Point::new(x, y);
        if cell == to {
            return true;
        }
        if !clear(cell) {
            return false;
        }
    }
}

/// What a unit can see: which way it faces, and who it saw at its last look.
///
/// The heading is inline, since it is kept up every tick; who was seen is a
/// fixed array **boxed once, at spawn**, since it is only touched every 7th
/// tick and inline it would more than double the size of a human — the
/// arena's hot data is what walks every tick, and this is not that. Looking
/// allocates nothing.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Perception {
    heading: Heading,
    seen: Box<[Option<Sighting>; SIGHTINGS]>,
    /// How many of `seen` are filled, from the front.
    len: u8,
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
    pub fn look(&mut self, ctx: &Think<'_>, body: &Body) {
        // Only `len` is reset: what lies past it is never read, and clearing
        // the whole array on every look is a write nobody needs.
        self.len = 0;
        let here = body.center_position();
        let me = body.uid();
        for cell in cone(self.heading) {
            if self.len as usize == SIGHTINGS {
                break;
            }
            let at = here + cell.offset;
            let Some(uid) = ctx.occupancy.occupant(at) else {
                continue;
            };
            if uid == me || !in_sight(ctx.map, here, at) {
                continue;
            }
            self.seen[self.len as usize] = Some(Sighting { uid, cell: at, range: cell.range });
            self.len += 1;
        }
    }

    /// See nobody: what a look is while asleep.
    pub fn close_eyes(&mut self) {
        self.len = 0;
    }

    /// Who was seen at the last look, nearest first.
    pub fn seen(&self) -> impl Iterator<Item = Sighting> + '_ {
        self.seen[..self.len as usize].iter().flatten().copied()
    }

    /// For the brains menu. Allocates; never asked in a tick.
    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let near = self.seen().filter(|seen| seen.range == Range::Near).count();
        let far = self.seen().filter(|seen| seen.range == Range::Far).count();
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

    /// A unit at `cell` facing `heading` in `world`, having just looked.
    fn look(world: &World, cell: Point, heading: Heading) -> Perception {
        let (dx, dy) = heading.step();
        let mut perception = Perception::default();
        perception.turn(Some((dx as f32, dy as f32)));
        perception.look(&world.ctx(), &Body::at_cell(uid(1), cell));
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
            let cells = cone(heading);
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
            .iter()
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
        let seen: Vec<Sighting> = look(&world, me, Heading::North).seen().collect();
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
        assert_eq!(look(&world, me, Heading::North).seen().count(), 0);
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
        let seen: Vec<Uid> = look(&world, me, Heading::North).seen().map(|s| s.uid).collect();
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
        assert!(!in_sight(&world.map, me, Point::new(6, 6)), "even right beside it");
        assert_eq!(look(&world, me, Heading::NorthEast).seen().count(), 0);
        // With a gap in it there is a view.
        world.map.set_terrain(Point::new(6, 5), FLOOR);
        assert!(in_sight(&world.map, me, Point::new(6, 6)));
    }

    #[test]
    fn in_a_crowd_only_the_nearest_are_taken_in() {
        let mut world = World::new(Map::new(Size::new(30, 30), FLOOR));
        let me = Point::new(15, 5);
        let mut n = 2;
        for cell in cone(Heading::North) {
            put(&mut world, n, me + cell.offset);
            n += 1;
        }
        let perception = look(&world, me, Heading::North);
        let seen: Vec<Sighting> = perception.seen().collect();
        assert_eq!(seen.len(), SIGHTINGS);
        let nearest: Vec<Point> = cone(Heading::North)[..SIGHTINGS].iter().map(|c| me + c.offset).collect();
        assert_eq!(seen.iter().map(|s| s.cell).collect::<Vec<_>>(), nearest);
    }

    #[test]
    fn a_look_forgets_what_the_last_one_saw() {
        let mut world = World::new(Map::new(Size::new(20, 20), FLOOR));
        let me = Point::new(10, 5);
        put(&mut world, 2, Point::new(10, 7));
        let mut perception = look(&world, me, Heading::North);
        assert_eq!(perception.seen().count(), 1);
        world.occupancy.clear();
        perception.look(&world.ctx(), &Body::at_cell(uid(1), me));
        assert_eq!(perception.seen().count(), 0);
    }
}
