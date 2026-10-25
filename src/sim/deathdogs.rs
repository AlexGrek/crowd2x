//! Deathdogs: what comes out of the wasteland at night.
//!
//! **They come at dusk and go at dawn.** On the tick it gets dark — the
//! street lights' photocell, `Sun::is_dark` — [`PACKS`] packs of
//! [`PACK_SIZE`] come out of the wasteland round an endless map, each at a
//! random point [`SPAWN_DISTANCE`] out from the site ([`arrive`]); by day each
//! walks off (`brain::goals::DepartGoal`) and leaves the world once it is
//! [`DEPART_DISTANCE`] out (`kinds::Deathdog::react`, `Effect::Depart`).
//!
//! **They never set foot on the site.** A route for one goes round it
//! (`Think::step_cost_with`), the move step refuses it a step in, and the
//! spawn pass will not put one there. So **inside is safe**: they hunt only
//! whoever is outside the walls, and bite only whoever is outside
//! (`brain::tasks::Bite`, `Effect::Bite`) for [`BITE_DAMAGE`].
//!
//! The rest is the brain's: what a deathdog sees and hunts, and how anybody
//! outside who sees one — or sees somebody running from one — panics and
//! runs for a way in (`brain::alarm`, `sim::panics`, `sim::refuge`).

use std::ops::RangeInclusive;

use rand::RngExt;

use crate::map::{Map, Point};

use super::clock::{watched, Clock};
use super::uid::EntityType;
use super::GameState;

/// How many packs come out a night.
pub const PACKS: usize = 2;

/// How many deathdogs to a pack.
pub const PACK_SIZE: usize = 3;

/// How far out from the site, in cells, a pack comes out of the wasteland.
pub const SPAWN_DISTANCE: i32 = 48;

/// How far round a pack's point its deathdogs come out, in cells either way.
pub const PACK_SPREAD: i32 = 3;

/// How far out from the site a deathdog walking off by day has to be to have
/// gone: as far out as they come from.
pub const DEPART_DISTANCE: i32 = SPAWN_DISTANCE;

/// How near the site, in cells, a prowling deathdog comes...
pub const LURK_NEAR: i32 = 3;

/// ...and how far from it it strays.
pub const LURK_FAR: i32 = 24;

/// How much health a bite takes.
pub const BITE_DAMAGE: RangeInclusive<u8> = 20..=60;

/// How long a deathdog takes between two bites, in **watched** seconds: a
/// second of watching, two minutes of world.
pub const BITE_INTERVAL: f32 = watched(super::clock::MINUTE * 2.0);

/// How many cells `cell` is from the site, the steps a king would take:
/// 0 on it.
pub fn distance_from_site(map: &Map, cell: Point) -> i32 {
    map.site().chebyshev_distance(cell)
}

/// Whether `before` to `now` is the tick it got dark.
pub fn is_dusk(before: Clock, now: Clock) -> bool {
    !before.sun().is_dark() && now.sun().is_dark()
}

/// **The night's arrivals**, on the tick it gets dark, on an endless map
/// only: [`PACKS`] packs of [`PACK_SIZE`], each at a random point on the ring
/// [`SPAWN_DISTANCE`] out from the site, its deathdogs on free, passable,
/// open ground within [`PACK_SPREAD`] of it. The dice are the world's own
/// ([`GameState`]'s saved rng), so a run comes out the same every time.
///
/// Called at the end of a processing pass (`sim::comings_and_goings`), once
/// the frozen table is let go — it adds to the crowd.
pub(super) fn arrive(state: &mut GameState) {
    if !state.map.is_endless() {
        return;
    }
    let ring = state.map.site().grown(SPAWN_DISTANCE);
    let (width, height) = (ring.size.width, ring.size.height);
    let perimeter = 2 * (width + height) - 4;
    for _ in 0..PACKS {
        let along = state.rng.random_range(0..perimeter);
        let point = on_ring(ring.origin, width, height, along);
        let mut came = 0;
        for _ in 0..PACK_SIZE {
            let Some(cell) = free_ground_near(state, point) else {
                break;
            };
            state.spawn(EntityType::Deathdog, cell);
            came += 1;
        }
        state.log.important(match came {
            0 => format!("no deathdogs could come out of the wasteland at {}, {}", point.x, point.y),
            1 => format!("a deathdog came out of the wasteland at {}, {}", point.x, point.y),
            n => format!("a pack of {n} deathdogs came out of the wasteland at {}, {}", point.x, point.y),
        });
    }
}

/// The `along`th cell round the edge of a ring of bounds at `origin`,
/// `width` by `height`, going round from its bottom-left corner.
fn on_ring(origin: Point, width: i32, height: i32, along: i32) -> Point {
    let (w, h) = (width - 1, height - 1);
    let (dx, dy) = if along < w {
        (along, 0)
    } else if along < w + h {
        (w, along - w)
    } else if along < 2 * w + h {
        (w - (along - w - h), h)
    } else {
        (0, h - (along - 2 * w - h))
    };
    origin.offset(dx, dy)
}

/// A cell within [`PACK_SPREAD`] of `point` a deathdog may come out in:
/// passable, nobody in it, off the site and open wasteland (not a pocket a
/// lake walls in). A few random tries, then every cell in a fixed order;
/// `None` when there is none.
fn free_ground_near(state: &mut GameState, point: Point) -> Option<Point> {
    let fits = |state: &GameState, cell: Point| {
        state.map.is_passable(cell)
            && !state.map.site().contains(cell)
            && state.occupancy.occupant(cell).is_none()
            && state.map.navigation().is_outside(cell)
    };
    for _ in 0..8 {
        let cell = point.offset(
            state.rng.random_range(-PACK_SPREAD..=PACK_SPREAD),
            state.rng.random_range(-PACK_SPREAD..=PACK_SPREAD),
        );
        if fits(state, cell) {
            return Some(cell);
        }
    }
    (-PACK_SPREAD..=PACK_SPREAD)
        .flat_map(|dy| (-PACK_SPREAD..=PACK_SPREAD).map(move |dx| point.offset(dx, dy)))
        .find(|&cell| fits(state, cell))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_is_walked_round_every_cell_once() {
        let origin = Point::new(-2, -3);
        let (width, height) = (5, 4);
        let cells: Vec<Point> = (0..2 * (width + height) - 4).map(|along| on_ring(origin, width, height, along)).collect();
        let mut unique = cells.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), cells.len());
        let bounds = crate::map::Bounds::new(origin, crate::map::Size::new(width, height));
        assert!(cells.iter().all(|&cell| bounds.on_edge(cell)), "{cells:?}");
    }
}
