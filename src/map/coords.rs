//! Grid coordinates.
//!
//! Everything that addresses the map does it in whole cells, as `i32`. Not
//! `usize`, because half of the arithmetic a simulation does with a cell is
//! relative — a neighbour, a delta, a direction — and those go negative
//! constantly; a `usize` grid turns every one of those into a cast with an
//! underflow waiting in it. And not `f32`: a cell has an exact identity, it is
//! a key in a grid and a node in a path, so comparing and hashing it has to be
//! exact.
//!
//! World-space floats stay on the Bevy side of the fence. A [`Point`] is a
//! cell, and the adapter that draws the map is what multiplies it out into
//! canvas pixels.

use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

/// A cell address.
///
/// Y increases upward, the same direction as world Y, so the drawing adapter
/// is a scale and never a flip.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[derive(Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const ORIGIN: Point = Point::new(0, 0);

    /// The four steps that share an edge with a cell, in a fixed order.
    ///
    /// The order is part of the contract: a search that visits neighbours in
    /// id order is deterministic, and determinism is what makes a crowd
    /// reproducible from a seed.
    pub const CARDINALS: [Point; 4] = [
        Point::new(1, 0),
        Point::new(0, 1),
        Point::new(-1, 0),
        Point::new(0, -1),
    ];

    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }

    /// The four edge-sharing neighbours, in [`Point::CARDINALS`] order.
    pub fn cardinal_neighbours(self) -> [Point; 4] {
        Self::CARDINALS.map(|step| self + step)
    }

    /// Steps needed to walk there on a 4-connected grid — the admissible A*
    /// heuristic for cardinal movement.
    ///
    /// `abs_diff` rather than a subtraction so the intermediate difference
    /// cannot overflow and panic a debug build; only a distance larger than
    /// `i32::MAX`, which no map comes near, is misreported.
    pub fn manhattan_distance(self, other: Point) -> i32 {
        (self.x.abs_diff(other.x) + self.y.abs_diff(other.y)) as i32
    }

    /// Steps on an 8-connected grid, where a diagonal costs the same as a step.
    pub fn chebyshev_distance(self, other: Point) -> i32 {
        self.x.abs_diff(other.x).max(self.y.abs_diff(other.y)) as i32
    }
}

impl Add for Point {
    type Output = Point;

    fn add(self, rhs: Point) -> Point {
        Point::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Point {
    type Output = Point;

    fn sub(self, rhs: Point) -> Point {
        Point::new(self.x - rhs.x, self.y - rhs.y)
    }
}

/// Map dimensions, in cells.
///
/// Sizes are `i32` like everything else, so a size and a coordinate can be
/// compared without a cast at each call site. The constructor is where that
/// choice is paid for: it rejects the sizes the type allows but the format
/// does not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    /// Panics on a size no map can have — a non-positive side, or an area that
    /// does not fit in `i32`.
    ///
    /// For a size written in the source that is the right trade: it is a bug
    /// in the caller, and failing at construction keeps every later index
    /// total. A size that came off disk goes through [`Size::try_new`]
    /// instead; a corrupt file must not be able to panic the game.
    pub fn new(width: i32, height: i32) -> Self {
        Self::try_new(width, height)
            .unwrap_or_else(|| panic!("{width}x{height} is not a valid map size"))
    }

    /// `None` for a non-positive side or an area that does not fit in `i32`.
    pub fn try_new(width: i32, height: i32) -> Option<Self> {
        (width > 0 && height > 0 && width.checked_mul(height).is_some())
            .then_some(Self { width, height })
    }

    pub const fn area(self) -> usize {
        (self.width as i64 * self.height as i64) as usize
    }

    pub const fn contains(self, point: Point) -> bool {
        point.x >= 0 && point.y >= 0 && point.x < self.width && point.y < self.height
    }

    /// Row-major index of a cell, or `None` if it is off the map.
    ///
    /// This is the single place a coordinate becomes an offset into a
    /// grid's backing array; the layers and the passability map all go
    /// through it, so they cannot disagree about the layout.
    pub const fn index_of(self, point: Point) -> Option<usize> {
        if self.contains(point) {
            Some((point.y as i64 * self.width as i64 + point.x as i64) as usize)
        } else {
            None
        }
    }

    /// Inverse of [`Size::index_of`], for walking a backing array back out
    /// into coordinates.
    pub const fn point_at(self, index: usize) -> Point {
        let width = self.width as usize;
        Point::new((index % width) as i32, (index / width) as i32)
    }

    /// Every cell, row-major — the order the layers are stored in.
    pub fn points(self) -> impl Iterator<Item = Point> {
        (0..self.height).flat_map(move |y| (0..self.width).map(move |x| Point::new(x, y)))
    }
}

/// A rectangle of cells anywhere in the world: a [`Size`] that does not start
/// at the origin.
///
/// The authored map — the *site* — is a [`Size`] from `(0, 0)`, and most
/// layers are laid out over exactly that. The world round it is not: the
/// wasteland ([`super::wasteland`]) runs off in every direction, negative
/// coordinates included, so anything that has to cover some of it — a route
/// search's window, the area reachability is worked out over, a camera's view
/// — is one of these. Like [`Size::index_of`], [`Bounds::index_of`] is the
/// one place a cell becomes an offset into an array laid over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bounds {
    /// The bottom-left cell, inside.
    pub origin: Point,
    pub size: Size,
}

impl From<Size> for Bounds {
    fn from(size: Size) -> Bounds {
        Bounds { origin: Point::ORIGIN, size }
    }
}

impl Bounds {
    pub const fn new(origin: Point, size: Size) -> Bounds {
        Bounds { origin, size }
    }

    /// The smallest bounds holding every one of `cells`; `None` for none.
    pub fn around(cells: impl IntoIterator<Item = Point>) -> Option<Bounds> {
        let mut cells = cells.into_iter();
        let first = cells.next()?;
        let (min, max) = cells.fold((first, first), |(min, max), cell| {
            (
                Point::new(min.x.min(cell.x), min.y.min(cell.y)),
                Point::new(max.x.max(cell.x), max.y.max(cell.y)),
            )
        });
        Some(Bounds::from_corners(min, max))
    }

    /// From the bottom-left and top-right cells, both inside.
    pub fn from_corners(min: Point, max: Point) -> Bounds {
        Bounds {
            origin: min,
            size: Size::new(max.x - min.x + 1, max.y - min.y + 1),
        }
    }

    /// The top-right cell, inside.
    pub const fn max(self) -> Point {
        Point::new(self.origin.x + self.size.width - 1, self.origin.y + self.size.height - 1)
    }

    pub const fn area(self) -> usize {
        self.size.area()
    }

    pub const fn contains(self, point: Point) -> bool {
        self.size.contains(Point::new(point.x - self.origin.x, point.y - self.origin.y))
    }

    /// Row-major index of a cell, or `None` outside.
    pub const fn index_of(self, point: Point) -> Option<usize> {
        self.size.index_of(Point::new(point.x - self.origin.x, point.y - self.origin.y))
    }

    /// Inverse of [`Bounds::index_of`].
    pub const fn point_at(self, index: usize) -> Point {
        let local = self.size.point_at(index);
        Point::new(local.x + self.origin.x, local.y + self.origin.y)
    }

    /// Every cell, row-major.
    pub fn points(self) -> impl Iterator<Item = Point> {
        let origin = self.origin;
        self.size.points().map(move |p| p + origin)
    }

    /// Grown by `margin` cells on every side.
    pub fn grown(self, margin: i32) -> Bounds {
        Bounds::from_corners(self.origin.offset(-margin, -margin), self.max().offset(margin, margin))
    }

    /// The smallest bounds holding both.
    pub fn union(self, other: Bounds) -> Bounds {
        let (a, b) = (self.max(), other.max());
        Bounds::from_corners(
            Point::new(self.origin.x.min(other.origin.x), self.origin.y.min(other.origin.y)),
            Point::new(a.x.max(b.x), a.y.max(b.y)),
        )
    }

    /// Whether the two share a cell.
    pub fn overlaps(self, other: Bounds) -> bool {
        let (a, b) = (self.max(), other.max());
        self.origin.x <= b.x && other.origin.x <= a.x && self.origin.y <= b.y && other.origin.y <= a.y
    }

    /// Whether `cell` is on the outermost ring of cells inside.
    pub fn on_edge(self, cell: Point) -> bool {
        let max = self.max();
        self.contains(cell)
            && (cell.x == self.origin.x || cell.y == self.origin.y || cell.x == max.x || cell.y == max.y)
    }

    /// How many steps a king would take from `cell` to the nearest cell
    /// inside: 0 inside, 1 on the ring just outside.
    pub fn chebyshev_distance(self, cell: Point) -> i32 {
        let max = self.max();
        let dx = (self.origin.x - cell.x).max(cell.x - max.x).max(0);
        let dy = (self.origin.y - cell.y).max(cell.y - max.y).max(0);
        dx.max(dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_round_trips_through_a_point() {
        let size = Size::new(7, 5);
        for index in 0..size.area() {
            assert_eq!(size.index_of(size.point_at(index)), Some(index));
        }
    }

    #[test]
    fn cells_off_the_map_have_no_index() {
        let size = Size::new(4, 3);
        // Negative coordinates are the reason cells are addressed as i32 at
        // all; they must not wrap into a valid index.
        for outside in [
            Point::new(-1, 0),
            Point::new(0, -1),
            Point::new(4, 0),
            Point::new(0, 3),
        ] {
            assert_eq!(size.index_of(outside), None, "{outside:?}");
        }
    }

    #[test]
    fn points_walks_every_cell_in_storage_order() {
        let size = Size::new(3, 2);
        let walked: Vec<Point> = size.points().collect();
        assert_eq!(walked.len(), size.area());
        for (index, point) in walked.into_iter().enumerate() {
            assert_eq!(size.index_of(point), Some(index));
        }
    }

    #[test]
    fn neighbours_are_the_four_edge_sharing_cells() {
        let neighbours = Point::new(2, 3).cardinal_neighbours();
        assert_eq!(
            neighbours,
            [
                Point::new(3, 3),
                Point::new(2, 4),
                Point::new(1, 3),
                Point::new(2, 2),
            ]
        );
    }

    #[test]
    fn bounds_index_their_own_cells_wherever_they_start() {
        let bounds = Bounds::new(Point::new(-3, 5), Size::new(4, 3));
        assert_eq!(bounds.max(), Point::new(0, 7));
        for index in 0..bounds.area() {
            assert_eq!(bounds.index_of(bounds.point_at(index)), Some(index));
        }
        assert_eq!(bounds.points().count(), 12);
        assert!(bounds.contains(Point::new(-3, 5)) && bounds.contains(Point::new(0, 7)));
        assert_eq!(bounds.index_of(Point::new(1, 5)), None);
        assert_eq!(bounds.index_of(Point::new(-4, 5)), None);
    }

    #[test]
    fn bounds_grow_join_and_measure_distance() {
        let site = Bounds::from(Size::new(10, 5));
        assert_eq!(site.grown(2), Bounds::from_corners(Point::new(-2, -2), Point::new(11, 6)));
        let far = Bounds::around([Point::new(20, -4), Point::new(22, -1)]).unwrap();
        assert!(!site.overlaps(far));
        assert_eq!(site.union(far), Bounds::from_corners(Point::new(0, -4), Point::new(22, 4)));
        assert_eq!(site.chebyshev_distance(Point::new(3, 3)), 0);
        assert_eq!(site.chebyshev_distance(Point::new(-1, 2)), 1);
        assert_eq!(site.chebyshev_distance(Point::new(12, 9)), 5);
        assert!(site.on_edge(Point::new(0, 3)) && !site.on_edge(Point::new(1, 3)));
    }

    #[test]
    fn distances_are_measured_in_steps() {
        let a = Point::new(1, 1);
        let b = Point::new(4, 5);
        assert_eq!(a.manhattan_distance(b), 7);
        assert_eq!(a.chebyshev_distance(b), 4);
        assert_eq!(a.manhattan_distance(b), b.manhattan_distance(a));
    }
}
