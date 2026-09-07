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
    fn distances_are_measured_in_steps() {
        let a = Point::new(1, 1);
        let b = Point::new(4, 5);
        assert_eq!(a.manhattan_distance(b), 7);
        assert_eq!(a.chebyshev_distance(b), 4);
        assert_eq!(a.manhattan_distance(b), b.manhattan_distance(a));
    }
}
