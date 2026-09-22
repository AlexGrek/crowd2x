//! What is on the canvas, and what is near enough to it to matter.
//!
//! The canvas is `window_physical / zoom` world units — 320x180 at the default
//! zoom on the default window, which is under seven cells by four. Everything
//! that draws reads this module to find out which small part of the world it
//! has to build sprites for, so the number of sprite entities in the world is a
//! function of the canvas rather than of the map or of the crowd.
//!
//! **Why this is not solved with [`Visibility`].** Bevy's `extract_sprites`
//! walks every sprite entity in the main world each frame and only *then* skips
//! the invisible ones; `calculate_bounds_2d` and the visibility check walk them
//! too. A hidden sprite is cheaper than a visible one and it is not free, so the
//! only thing that actually scales is having fewer entities — which is what
//! culling to [`VisibleArea`] buys, and why every pool built on it has to stay
//! bounded by the view rather than growing to fit the crowd.
//!
//! # Three rects, because they answer three questions
//!
//! * [`VisibleArea::canvas`] is exactly what is rendered. Nothing culls against
//!   it directly — a sprite is centred on its position, so something just
//!   outside still has pixels inside.
//! * [`VisibleArea::actors`] is `canvas` grown by [`characters`]' overhang: the
//!   rect a unit's *position* has to be in for any part of it, or of the bar and
//!   label drawn over it, to land on the canvas. This is what decides that a
//!   unit gets a sprite.
//! * [`VisibleArea::actors_keep`] is `actors` grown by [`HYSTERESIS`]: a unit
//!   that already has a sprite keeps it until it leaves this. Taking and parking
//!   a sprite moves it between archetypes, so a unit pacing the boundary would
//!   otherwise churn every frame.
//!
//! and [`VisibleArea::tiles`] is the same question for the terrain grid, in
//! cells. Tiles need no hysteresis: the cell rect only changes when the camera
//! crosses a cell boundary, which already *is* a band of a whole cell.
//!
//! # Ordering
//!
//! [`update_view`] runs `.after(render::PixelSystems)`, which is not tidiness.
//! It reads the canvas's size and the camera's snapped position, and both are
//! written in that set: a view computed before `resize_canvas` is a zoom step
//! behind, and one computed before `snap_camera_to_pixels` is half a pixel out.
//! Either shows up as an unpainted strip at the leading edge of a fast pan.
//!
//! Everything that culls runs `.after(ViewSystems)`.

use bevy::prelude::*;

use crate::characters::{OVERHANG_ABOVE, OVERHANG_BELOW, OVERHANG_SIDE};
use crate::editor::background::TILE;
use crate::render::{PixelCanvas, PixelSystems, WorldCamera};

/// How far a unit must travel past [`VisibleArea::actors`] before it loses the
/// sprite it already has.
///
/// One cell. Taking a sprite from a pool inserts `Actor` and parking it removes
/// that component again, and each of those moves the entity between archetypes,
/// so a unit standing exactly on the edge would pay for it every frame. At a
/// walking pace of about a cell a second this is a second of dead zone.
pub const HYSTERESIS: f32 = TILE;

/// An inclusive rectangle of whole cells.
///
/// Ours rather than [`IRect`] because the inclusivity has to be stated once and
/// obeyed everywhere: Bevy's leaves it to whichever method you reach for, and an
/// off-by-one here is a one-cell strip of unpainted map down the edge of the
/// screen, which reads as a rendering bug rather than as a rounding one.
///
/// `max` is *inside* the rectangle. An empty rect is `max < min` on either axis,
/// which is what [`Self::EMPTY`] is and what a default-constructed one is, so a
/// window that has not been filled yet covers nothing rather than covering cell
/// zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    pub min: IVec2,
    pub max: IVec2,
}

impl Default for CellRect {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl CellRect {
    /// Covers nothing at all — not cell `(0, 0)`.
    pub const EMPTY: Self = Self {
        min: IVec2::new(0, 0),
        max: IVec2::new(-1, -1),
    };

    pub fn new(min: IVec2, max: IVec2) -> Self {
        Self { min, max }
    }

    pub fn is_empty(&self) -> bool {
        self.max.x < self.min.x || self.max.y < self.min.y
    }

    pub fn contains(&self, cell: IVec2) -> bool {
        cell.x >= self.min.x
            && cell.x <= self.max.x
            && cell.y >= self.min.y
            && cell.y <= self.max.y
    }

    /// How many cells it covers. `usize`, because every caller is sizing
    /// something.
    pub fn area(&self) -> usize {
        if self.is_empty() {
            return 0;
        }
        let w = (self.max.x - self.min.x + 1) as usize;
        let h = (self.max.y - self.min.y + 1) as usize;
        w * h
    }

    /// Every cell, in row-major order — the same order `map::Size::points`
    /// walks, so a window filled from this touches memory the way the map is
    /// laid out.
    pub fn cells(&self) -> impl Iterator<Item = IVec2> + '_ {
        let (min, max) = (self.min, self.max);
        (min.y..=max.y).flat_map(move |y| (min.x..=max.x).map(move |x| IVec2::new(x, y)))
    }

    /// The part of `self` that is also in `other`.
    pub fn intersect(&self, other: Self) -> Self {
        if self.is_empty() || other.is_empty() {
            return Self::EMPTY;
        }
        Self {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
    }
}

/// What is on the canvas this frame.
///
/// See the module docs for why there are three world rects and not one.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct VisibleArea {
    /// False until there is both a camera and a canvas.
    ///
    /// `OnEnter` for the *initial* state runs before every `Startup` system, so
    /// a screen booted into directly reaches its own setup before
    /// `render::setup_pipeline` has spawned either. Nothing that draws may run
    /// while this is false; doing so would cull everything against a rect of
    /// zeroes and show an empty world.
    pub ready: bool,
    /// Exactly what the canvas covers, in world units.
    pub canvas: Rect,
    /// Where a unit's position has to be for it to be worth drawing.
    pub actors: Rect,
    /// Where a unit already drawn has to leave before it stops being drawn.
    pub actors_keep: Rect,
    /// Cells whose tile art can put a pixel on the canvas.
    pub tiles: CellRect,
}

impl VisibleArea {
    /// Should a unit at `pos` (world units) be given a sprite?
    pub fn should_draw(&self, pos: Vec2) -> bool {
        self.ready && self.actors.contains(pos)
    }

    /// Should a unit at `pos` that already has one keep it?
    pub fn should_keep(&self, pos: Vec2) -> bool {
        self.ready && self.actors_keep.contains(pos)
    }
}

/// Marks the system that works out [`VisibleArea`]. Everything that culls
/// against it runs `.after` this.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSystems;

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        // `init_resource`, never inserted in `OnEnter`: a screen that reads this
        // has to find it whatever order the screens were entered in.
        app.init_resource::<VisibleArea>().add_systems(
            Update,
            update_view.in_set(ViewSystems).after(PixelSystems),
        );
    }
}

/// The canvas as a world rect.
///
/// `canvas` is the canvas resolution, which *is* its size in world units: the
/// world camera renders at one world unit to one canvas pixel, which is the
/// whole basis of the pixel pipeline. `centre` is the camera, already rounded
/// to whole pixels.
///
/// Note this is deliberately not built from `render::half_view`, which measures
/// the *logical* window and does not round up the way `canvas_size_for` does —
/// the two differ by up to half a pixel on an odd window, and the rect has to
/// match what was actually rendered.
pub fn canvas_rect(canvas: UVec2, centre: Vec2) -> Rect {
    let half = canvas.as_vec2() / 2.0;
    Rect::from_corners(centre - half, centre + half)
}

/// Grow a rect by the overhang of what is drawn about a character.
pub fn grown_for_actors(canvas: Rect) -> Rect {
    Rect::from_corners(
        Vec2::new(canvas.min.x - OVERHANG_SIDE, canvas.min.y - OVERHANG_BELOW),
        Vec2::new(canvas.max.x + OVERHANG_SIDE, canvas.max.y + OVERHANG_ABOVE),
    )
}

/// The cells a world rect touches, padded by one.
///
/// A tile is one cell wide once scaled and centred on its cell, so half a cell
/// of padding would do; one whole cell is the version with no arithmetic to get
/// wrong, and it costs about thirty entities.
///
/// `floor` and not `as i32`: truncation collapses the whole strip between
/// `-TILE` and `0` onto cell 0, which paints the wrong side of the axis — the
/// same trap `background::cell_of` already documents.
pub fn cells_covering(world: Rect) -> CellRect {
    let min = (world.min / TILE).floor().as_ivec2() - IVec2::ONE;
    let max = (world.max / TILE).floor().as_ivec2() + IVec2::ONE;
    CellRect::new(min, max)
}

fn update_view(
    mut area: ResMut<VisibleArea>,
    canvas: Option<Res<PixelCanvas>>,
    cameras: Query<&Transform, With<WorldCamera>>,
) {
    let (Some(canvas), Ok(camera)) = (canvas, cameras.single()) else {
        // Written through `set_if_neq`-style guarding so a frame with no camera
        // does not mark the resource changed over and over.
        if area.ready {
            area.ready = false;
        }
        return;
    };

    let centre = camera.translation.truncate();
    let canvas_rect = canvas_rect(canvas.size, centre);
    let actors = grown_for_actors(canvas_rect);
    let wanted = VisibleArea {
        ready: true,
        canvas: canvas_rect,
        actors,
        actors_keep: Rect::from_corners(
            actors.min - Vec2::splat(HYSTERESIS),
            actors.max + Vec2::splat(HYSTERESIS),
        ),
        tiles: cells_covering(canvas_rect),
    };

    // Compared before writing: `ResMut` marks the resource changed on any
    // mutable deref, and a camera that has not moved should not make every
    // consumer's `is_changed` fire.
    if area.canvas != wanted.canvas || !area.ready {
        *area = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characters::CELL;

    /// The 1280x720 window `main` asks for, at the default zoom.
    const CANVAS: UVec2 = UVec2::new(320, 180);

    #[test]
    fn the_canvas_rect_is_the_camera_plus_half_the_canvas() {
        let rect = canvas_rect(CANVAS, Vec2::new(288.0, 216.0));
        assert_eq!(rect.min, Vec2::new(128.0, 126.0));
        assert_eq!(rect.max, Vec2::new(448.0, 306.0));
    }

    /// `canvas_size_for` rounds *up*, so an odd canvas is a real case and the
    /// rect has to stay centred on the camera rather than favouring one edge.
    #[test]
    fn an_odd_canvas_stays_centred_on_the_camera() {
        let centre = Vec2::new(100.0, 50.0);
        let rect = canvas_rect(UVec2::new(201, 101), centre);
        assert_eq!(rect.min, Vec2::new(-0.5, -0.5));
        assert_eq!(rect.max, Vec2::new(200.5, 100.5));
        assert_eq!((rect.min + rect.max) / 2.0, centre);
    }

    #[test]
    fn cells_covering_includes_the_cell_under_every_corner() {
        // Exactly cells (0,0) to (1,1), before the one-cell pad.
        let rect = Rect::from_corners(Vec2::ZERO, Vec2::splat(TILE * 2.0));
        let cells = cells_covering(rect);
        // Padded by one on each side, and the far corner lands on the boundary
        // of cell 2, which `floor` puts *in* cell 2.
        assert_eq!(cells.min, IVec2::splat(-1));
        assert_eq!(cells.max, IVec2::splat(3));
        // Every corner of the unpadded rect is covered.
        for corner in [rect.min, rect.max, Vec2::new(rect.min.x, rect.max.y)] {
            let cell = (corner / TILE).floor().as_ivec2();
            assert!(cells.contains(cell), "{corner:?} -> {cell:?} not covered");
        }
    }

    /// Truncation instead of flooring would collapse the strip between -TILE
    /// and 0 onto cell 0 — the trap `background::cell_of` already guards.
    #[test]
    fn a_cell_rect_left_of_the_origin_rounds_down() {
        let rect = Rect::from_corners(Vec2::splat(-TILE - 1.0), Vec2::splat(-1.0));
        let cells = cells_covering(rect);
        assert_eq!(cells.min, IVec2::splat(-3));
        assert_eq!(cells.max, IVec2::splat(0));
        assert!(cells.contains(IVec2::splat(-2)));
    }

    /// A unit one cell above the canvas still has a goal label on it; two cells
    /// above it does not. The margin is what stops a label popping in.
    #[test]
    fn the_margin_reaches_a_goal_label() {
        let canvas = canvas_rect(CANVAS, Vec2::new(288.0, 216.0));
        let actors = grown_for_actors(canvas);
        let one_above = Vec2::new(288.0, canvas.max.y + CELL as f32);
        let two_above = Vec2::new(288.0, canvas.max.y + CELL as f32 * 2.0);
        assert!(actors.contains(one_above));
        assert!(!actors.contains(two_above));
        // ...and the sides are tighter than the top, because nothing is drawn
        // out to the side of a character.
        assert!(actors.max.y - canvas.max.y > actors.max.x - canvas.max.x);
    }

    #[test]
    fn the_keep_rect_always_contains_the_enter_rect() {
        for centre in [Vec2::ZERO, Vec2::new(288.0, 216.0), Vec2::new(-90.0, 7.5)] {
            let canvas = canvas_rect(CANVAS, centre);
            let actors = grown_for_actors(canvas);
            let keep = Rect::from_corners(
                actors.min - Vec2::splat(HYSTERESIS),
                actors.max + Vec2::splat(HYSTERESIS),
            );
            assert!(keep.contains(actors.min));
            assert!(keep.contains(actors.max));
            assert!(keep.min.x < actors.min.x && keep.max.y > actors.max.y);
        }
    }

    /// A unit standing exactly on the edge must not gain and lose its sprite
    /// every frame: once drawn, it keeps its sprite across the whole band.
    #[test]
    fn a_unit_pacing_the_boundary_keeps_its_sprite() {
        let mut area = VisibleArea::default();
        area.ready = true;
        area.canvas = canvas_rect(CANVAS, Vec2::new(288.0, 216.0));
        area.actors = grown_for_actors(area.canvas);
        area.actors_keep = Rect::from_corners(
            area.actors.min - Vec2::splat(HYSTERESIS),
            area.actors.max + Vec2::splat(HYSTERESIS),
        );

        let edge = area.actors.max.x;
        let mut drawn = true;
        // One texel either side of the boundary, back and forth.
        for step in 0..8 {
            let x = if step % 2 == 0 { edge - 3.0 } else { edge + 3.0 };
            let pos = Vec2::new(x, 216.0);
            drawn = if drawn {
                area.should_keep(pos)
            } else {
                area.should_draw(pos)
            };
            assert!(drawn, "lost its sprite one texel past the edge at step {step}");
        }
        // It does eventually go, a whole cell out.
        assert!(!area.should_keep(Vec2::new(edge + HYSTERESIS + 1.0, 216.0)));
    }

    #[test]
    fn a_rect_that_did_not_move_diffs_to_nothing() {
        let rect = CellRect::new(IVec2::new(0, 0), IVec2::new(5, 3));
        let left: Vec<_> = rect.cells().filter(|c| !rect.contains(*c)).collect();
        assert!(left.is_empty());
        assert_eq!(rect.area(), rect.cells().count());
    }

    #[test]
    fn a_rect_moved_one_cell_leaves_and_enters_one_column() {
        let old = CellRect::new(IVec2::new(0, 0), IVec2::new(5, 3));
        let new = CellRect::new(IVec2::new(1, 0), IVec2::new(6, 3));
        let left: Vec<_> = old.cells().filter(|c| !new.contains(*c)).collect();
        let entered: Vec<_> = new.cells().filter(|c| !old.contains(*c)).collect();
        // Four rows tall, one column each way.
        assert_eq!(left.len(), 4);
        assert_eq!(entered.len(), 4);
        assert!(left.iter().all(|c| c.x == 0));
        assert!(entered.iter().all(|c| c.x == 6));
    }

    #[test]
    fn two_rects_that_do_not_overlap_swap_every_cell() {
        let old = CellRect::new(IVec2::new(0, 0), IVec2::new(3, 3));
        let new = CellRect::new(IVec2::new(10, 10), IVec2::new(13, 13));
        assert_eq!(old.cells().filter(|c| !new.contains(*c)).count(), 16);
        assert_eq!(new.cells().filter(|c| !old.contains(*c)).count(), 16);
        assert!(old.intersect(new).is_empty());
    }

    #[test]
    fn an_empty_rect_covers_nothing_not_cell_zero() {
        let empty = CellRect::default();
        assert!(empty.is_empty());
        assert_eq!(empty.area(), 0);
        assert_eq!(empty.cells().count(), 0);
        assert!(!empty.contains(IVec2::ZERO));
    }

    /// The headline invariant, as arithmetic: however big the map and however
    /// far out the zoom, the number of tiles drawn is bounded by the canvas.
    #[test]
    fn the_number_of_cells_drawn_is_bounded_by_the_canvas() {
        for window in [UVec2::new(1280, 720), UVec2::new(1002, 602), UVec2::new(640, 480)] {
            for zoom in 1..=8u32 {
                let canvas = (window + zoom - 1) / zoom;
                let cells = cells_covering(canvas_rect(canvas, Vec2::new(4000.0, 4000.0)));
                let bound = (canvas.x as f32 / TILE + 3.0).ceil() as usize
                    * (canvas.y as f32 / TILE + 3.0).ceil() as usize;
                assert!(
                    cells.area() <= bound,
                    "{window:?} at zoom {zoom}: {} cells, bound {bound}",
                    cells.area()
                );
                // ...and it is genuinely small: the whole point.
                assert!(cells.area() < 1200, "{window:?} at zoom {zoom}");
            }
        }
    }
}
