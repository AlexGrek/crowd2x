//! The schema: every message a `.savegame` holds, as Rust structs.
//!
//! Written by hand with `prost`'s derives rather than generated from `.proto`
//! files, so there is no build script and no `protoc` — these structs *are*
//! the schema of record. They are Protobuf on the wire all the same, and they
//! evolve by Protobuf's rules, which are the whole reason the format is
//! Protobuf:
//!
//! * **A tag is forever.** Never renumber a field and never reuse a number,
//!   not even one whose field was removed; list removed tags in the struct's
//!   docs as reserved.
//! * **A new field must mean "as before" at its default** (zero, empty,
//!   `None`): that is what an old save reads as. An old build skips a tag it
//!   does not know.
//! * **A new file is optional.** A save made before it has none, and loading
//!   reads that as defaults ([`super::archive`]).
//! * **Content is stored by name** — terrain, props, items, entity kinds —
//!   since a catalogue is reordered freely ([`crate::map`] does the same).
//!   The game's own state machines (a heading, a goal, a task's result) are
//!   prost enumerations with explicit numbers, mapped to the game's enums by
//!   an exhaustive `match` in each direction and never by `as`, so a reordered
//!   Rust enum cannot change what a saved number means.
//!
//! Cell lists — routes, a path's patch, destinations — are packed pairs of
//! `sint32` (`x0, y0, x1, y1, ...`; [`pack`] and [`unpack`]): a route is
//! hundreds of cells, and a message per cell would be twice the bytes.

pub mod brain;
pub mod entity;
pub mod manifest;
pub mod map;
pub mod view;
pub mod world;

pub use brain::*;
pub use entity::*;
pub use manifest::*;
pub use map::*;
pub use view::*;
pub use world::*;

use crate::map::Point;

use super::Invalid;

/// One cell.
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Cell {
    #[prost(sint32, tag = "1")]
    pub x: i32,
    #[prost(sint32, tag = "2")]
    pub y: i32,
}

impl From<Point> for Cell {
    fn from(point: Point) -> Cell {
        Cell { x: point.x, y: point.y }
    }
}

impl From<Cell> for Point {
    fn from(cell: Cell) -> Point {
        Point::new(cell.x, cell.y)
    }
}

/// A required cell: `None` is a broken save, not the origin.
pub fn cell(cell: Option<Cell>, what: &str) -> Result<Point, Invalid> {
    cell.map(Point::from).ok_or_else(|| Invalid::new(format!("{what}: no cell")))
}

/// Cells as packed `x, y` pairs.
pub fn pack(cells: impl IntoIterator<Item = Point>) -> Vec<i32> {
    cells.into_iter().flat_map(|cell| [cell.x, cell.y]).collect()
}

/// Packed `x, y` pairs back into cells. An odd count is refused: half a cell
/// is a truncated list, not one to guess the end of.
pub fn unpack(xy: &[i32], what: &str) -> Result<Vec<Point>, Invalid> {
    if xy.len() % 2 != 0 {
        return Err(Invalid::new(format!("{what}: {} coordinates is not a list of cells", xy.len())));
    }
    Ok(xy.chunks_exact(2).map(|pair| Point::new(pair[0], pair[1])).collect())
}

/// A prost enumeration field's raw number as the enumeration, refusing a
/// number this build does not know.
pub fn known<E: TryFrom<i32>>(raw: i32, what: &str) -> Result<E, Invalid> {
    E::try_from(raw).map_err(|_| Invalid::new(format!("{what}: unknown value {raw}")))
}

/// A required message.
pub fn required<'a, T>(message: &'a Option<T>, what: &str) -> Result<&'a T, Invalid> {
    message.as_ref().ok_or_else(|| Invalid::new(format!("{what}: missing")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_pack_and_unpack_in_order() {
        let cells = vec![Point::new(1, -2), Point::new(-30000, 7), Point::new(0, 0)];
        assert_eq!(unpack(&pack(cells.clone()), "route").unwrap(), cells);
    }

    #[test]
    fn half_a_cell_is_refused() {
        assert!(unpack(&[1, 2, 3], "route").is_err());
    }
}
