//! What every simulated thing is: [`GameEntity`], and the [`Body`] each one
//! carries.
//!
//! # Positions are in cells, not pixels
//!
//! [`Body::position`] is world-space in **cell units** — `(3.5, 2.5)` is the
//! middle of cell `(3, 2)`. The simulation already addresses everything in
//! cells ([`Point`], [`crate::map::PassabilityMap`]), and how many screen
//! pixels a cell is drawn at is a fact about the art, which this module has no
//! business knowing. `src/map/` draws the same line — "art is not in here" —
//! and the Bevy adapter multiplies by the tile size on its way to a
//! `Transform`.
//!
//! [`GameEntity::center_position`] is the cell the entity is standing in,
//! which is `position.floor()`. It is derived rather than stored so the two
//! can never disagree about where something is.

use crate::map::Point;

use super::uid::{EntityType, Uid};
use super::Intent;

/// The state every entity has, whatever else it has.
///
/// Kept as one embedded struct rather than as trait methods over private
/// fields so that the apply phase can write a position through
/// [`GameEntity::body_mut`] without every kind reimplementing a setter.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Body {
    uid: Uid,
    /// World position in cell units. See the module docs.
    position: (f32, f32),
}

impl Body {
    pub fn new(uid: Uid, position: (f32, f32)) -> Body {
        Body { uid, position }
    }

    /// A body standing in the middle of `cell`.
    pub fn at_cell(uid: Uid, cell: Point) -> Body {
        Body::new(uid, (cell.x as f32 + 0.5, cell.y as f32 + 0.5))
    }

    pub fn uid(&self) -> Uid {
        self.uid
    }

    pub fn position(&self) -> (f32, f32) {
        self.position
    }

    /// The cell this body is standing in.
    ///
    /// `floor`, not `round`: a cell owns the half-open range from its own
    /// coordinate to the next, so every position in cell 3 answers 3 —
    /// including 3.0 exactly. Rounding would give cell 3 the right-hand half of
    /// cell 2 and make the boundary land in the middle of a tile.
    pub fn center_position(&self) -> Point {
        Point::new(
            self.position.0.floor() as i32,
            self.position.1.floor() as i32,
        )
    }

    /// Move to a world position, in cell units.
    pub fn set_position(&mut self, position: (f32, f32)) {
        self.position = position;
    }
}

/// Anything the simulation ticks.
///
/// `Send + Sync` because the think phase is meant to run across threads; it is
/// stated here rather than left to be discovered when someone first tries.
pub trait GameEntity: Send + Sync {
    fn body(&self) -> &Body;
    fn body_mut(&mut self) -> &mut Body;

    /// Decide what to do, reading only. The returned [`Intent`] is applied
    /// later, in a single-threaded pass — see [`super::process_game_state`].
    ///
    /// Nothing about the world may be mutated here, which is what makes the
    /// whole phase parallelisable without a lock.
    fn think(&self, ctx: &Think<'_>) -> Intent;

    /// Carry out what [`GameEntity::think`] decided.
    ///
    /// The default writes the position and nothing else. A kind overrides this
    /// to keep its own state in step — a dog turning to face the way it walks
    /// — rather than that consequence being encoded in the intent, which
    /// describes what an entity *wants*.
    fn apply(&mut self, intent: &Intent) {
        if let Intent::Move { to, .. } = intent {
            self.body_mut().set_position(*to);
        }
    }

    /// A short description for the log and for debugging.
    fn describe(&self) -> String {
        self.uid().to_string()
    }

    // --- derived, from the body ---

    fn uid(&self) -> Uid {
        self.body().uid()
    }

    /// What this is. Read from the id, so an entity cannot report a type its
    /// own id disagrees with.
    ///
    /// `None` only for an id whose tag this build does not know, which cannot
    /// happen for an entity it constructed itself.
    fn kind(&self) -> Option<EntityType> {
        self.uid().kind()
    }

    fn position(&self) -> (f32, f32) {
        self.body().position()
    }

    fn center_position(&self) -> Point {
        self.body().center_position()
    }
}

/// What an entity is allowed to see while it thinks: the world, read-only.
///
/// A struct rather than a pile of arguments so that adding something for
/// entities to look at — a spatial index, the other entities, the time of day
/// — does not change every `think` signature in the game.
pub struct Think<'a> {
    pub map: &'a crate::map::Map,
    pub log: &'a super::log::Log,
    /// Seconds since the previous tick.
    pub dt: f32,
    /// Which tick this is. Deterministic, so it is usable as an RNG seed
    /// alongside an entity's own id.
    pub tick: u64,
}

impl Think<'_> {
    /// Whether an entity could stand in `cell`.
    pub fn is_passable(&self, cell: Point) -> bool {
        self.map.is_passable(cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(x: f32, y: f32) -> Body {
        Body::new(Uid::new(EntityType::Human, 7), (x, y))
    }

    #[test]
    fn the_center_cell_is_the_one_the_position_is_inside() {
        assert_eq!(body(3.5, 2.5).center_position(), Point::new(3, 2));
        // The low edge belongs to the cell it opens, the high edge to the next.
        assert_eq!(body(3.0, 2.0).center_position(), Point::new(3, 2));
        assert_eq!(body(3.999, 2.999).center_position(), Point::new(3, 2));
        assert_eq!(body(4.0, 3.0).center_position(), Point::new(4, 3));
    }

    #[test]
    fn cells_left_of_the_origin_floor_away_from_zero() {
        // `as i32` truncates towards zero, which would give -0.5 the cell 0
        // and put two cells' worth of world into one. `floor` first is why.
        assert_eq!(body(-0.5, -0.5).center_position(), Point::new(-1, -1));
        assert_eq!(body(-1.5, -2.5).center_position(), Point::new(-2, -3));
    }

    #[test]
    fn at_cell_puts_a_body_in_the_middle_of_it() {
        let uid = Uid::new(EntityType::Dog, 3);
        let cell = Point::new(6, 4);
        let body = Body::at_cell(uid, cell);
        assert_eq!(body.position(), (6.5, 4.5));
        assert_eq!(body.center_position(), cell);
    }

    #[test]
    fn moving_a_body_moves_the_cell_it_reports() {
        let mut body = body(1.5, 1.5);
        body.set_position((9.25, 0.75));
        assert_eq!(body.center_position(), Point::new(9, 0));
    }
}
