//! The entities that exist: humans and dogs.
//!
//! Both are wanderers for now — pick a nearby cell that can be stood on, walk
//! to it, pick another. That is not the simulation, it is the smallest thing
//! that exercises every part of the tick: thinking against the map, an intent
//! crossing from the read phase to the write phase, and a position the
//! renderer has to keep up with.
//!
//! # Nothing here knows what a human looks like
//!
//! A [`Human`] has no hairstyle, no outfit and no sprite. Which PNGs it is
//! drawn from is a fact about the art, and the art lives in `src/characters/`;
//! all the simulation owes the renderer is a number that is the same every
//! time for this entity and different for the next one, which
//! [`GameEntity::appearance_seed`] already gives it.
//!
//! Same line for [`Dog`]: its [`Facing`] is this module's own enum rather than
//! `characters::dog::Facing`, because that one is a Bevy `Component` and
//! nothing here imports Bevy.

use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

use crate::map::Point;

use super::entity::{Body, GameEntity, Think};
use super::uid::{EntityType, Uid};
use super::Intent;

/// How far a wanderer will look for somewhere to go, in cells.
const WANDER_RADIUS: i32 = 6;

/// How many cells to try before giving up and standing still this tick.
const WANDER_TRIES: u32 = 8;

/// Close enough to a goal to call it arrived, in cells.
const ARRIVED: f32 = 0.1;

/// The furthest anything moves in one tick, in cells.
///
/// Passability is checked at the *destination*, so a step longer than a cell
/// would step straight over a wall and land legally on the far side — the
/// check would pass and the wall would not exist. Capping below one cell means
/// a step crosses at most one boundary per axis, which is what makes the
/// destination check sufficient.
///
/// At the fixed timestep nothing comes close to this (a dog covers 0.055 cells
/// a tick). It is here because `process_game_state` is public and takes
/// whatever `dt` it is handed, and "only correct at one timestep" is not a
/// property worth relying on.
///
/// It does not stop a diagonal step cutting the corner between two walls; that
/// needs swept collision, and there is no diagonal geometry in the maps yet.
const MAX_STEP: f32 = 0.5;

/// How often a stuck entity is allowed to mention it. Roughly a second at the
/// fixed timestep.
const STUCK_LOG_TICKS: u64 = 64;

const HUMAN_SPEED: f32 = 2.0;
const DOG_SPEED: f32 = 3.5;

/// Which way something is facing. The simulation's own, deliberately not
/// `characters::dog::Facing` — see the module docs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Facing {
    Left,
    Right,
}

/// The part of an entity that walks: shared by every kind, because "has a
/// position and somewhere it is going" is not specific to any of them.
#[derive(Clone, Copy, Debug)]
pub struct Walker {
    body: Body,
    /// Where it is heading, in cells. `None` means "decide next tick".
    goal: Option<Point>,
    /// Cells per second.
    speed: f32,
}

impl Walker {
    fn new(uid: Uid, cell: Point, speed: f32) -> Walker {
        Walker {
            body: Body::at_cell(uid, cell),
            goal: None,
            speed,
        }
    }

    /// Decide where to be at the end of this tick.
    ///
    /// Read-only, as [`GameEntity::think`] requires: the goal it picked comes
    /// back inside the [`Intent`] rather than being written here.
    fn think(&self, ctx: &Think<'_>) -> Intent {
        // Somewhere to go, either the standing order or a fresh one. The RNG
        // is built inside the second arm rather than above the match: an
        // entity that already has a goal needs no randomness, and that is the
        // common case by a wide margin — seeding one per entity per tick to
        // then not use it is the sort of waste that only shows up at ten
        // thousand of them.
        let goal = match self.goal {
            Some(goal) if ctx.is_passable(goal) => goal,
            _ => {
                let mut rng = tick_rng(self.body.uid(), ctx.tick);
                match self.pick_goal(ctx, &mut rng) {
                    Some(goal) => goal,
                    // Walled in, or standing on a map with nothing walkable.
                    None => {
                        self.report_stuck(ctx);
                        return Intent::Idle;
                    }
                }
            }
        };

        let (x, y) = self.body.position();
        let (gx, gy) = (goal.x as f32 + 0.5, goal.y as f32 + 0.5);
        let (dx, dy) = (gx - x, gy - y);
        let distance = (dx * dx + dy * dy).sqrt();

        if distance <= ARRIVED {
            // Arrived: stop on the spot and choose again next tick.
            return Intent::Move {
                to: (gx, gy),
                goal: None,
            };
        }

        let step = (self.speed * ctx.dt).min(distance).min(MAX_STEP);
        let to = (x + dx / distance * step, y + dy / distance * step);

        // Refuse to walk into a wall rather than sliding along it: steering is
        // a later problem, and a wanderer that stops and re-picks is honest
        // about not having any yet.
        if !ctx.is_passable(cell_of(to)) {
            return Intent::Move {
                to: (x, y),
                goal: None,
            };
        }

        Intent::Move {
            to,
            goal: Some(goal),
        }
    }

    /// A passable cell within [`WANDER_RADIUS`], or `None` if a few tries
    /// found nothing.
    ///
    /// Bounded tries rather than a scan: this runs per idle entity per tick,
    /// and a bounded miss costs one wasted tick while a scan of the
    /// neighbourhood would cost the frame.
    fn pick_goal(&self, ctx: &Think<'_>, rng: &mut SmallRng) -> Option<Point> {
        let here = self.body.center_position();
        for _ in 0..WANDER_TRIES {
            let candidate = Point::new(
                here.x + rng.random_range(-WANDER_RADIUS..=WANDER_RADIUS),
                here.y + rng.random_range(-WANDER_RADIUS..=WANDER_RADIUS),
            );
            if candidate != here && ctx.is_passable(candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// Say so when there is nowhere to go, at most once a second.
    ///
    /// This is the only thing written from the think phase, and it is here to
    /// be exactly that: a `&Log` shared by every thinking entity, appended to
    /// without a lock. Rate-limited on the tick rather than on a stored
    /// counter because think may not mutate — and unlimited it would be one
    /// line per stuck entity per tick, which is how a log becomes noise.
    fn report_stuck(&self, ctx: &Think<'_>) {
        if ctx.tick.is_multiple_of(STUCK_LOG_TICKS) {
            ctx.log
                .push(format!("{} has nowhere to go", self.body.uid()));
        }
    }

    fn apply(&mut self, intent: &Intent) {
        if let Intent::Move { to, goal } = intent {
            self.body.set_position(*to);
            self.goal = *goal;
        }
    }
}

/// The cell a world position in cell units falls in.
///
/// The same rule as [`Body::center_position`], for a position that is not on a
/// body yet.
fn cell_of((x, y): (f32, f32)) -> Point {
    Point::new(x.floor() as i32, y.floor() as i32)
}

/// A deterministic RNG for one entity on one tick.
///
/// Per-entity and seeded, so the simulation replays identically from the same
/// starting state — and, more usefully, so the think phase needs no shared
/// mutable RNG, which is the one thing that would stop it parallelising.
fn tick_rng(uid: Uid, tick: u64) -> SmallRng {
    // Mixed rather than added: neighbouring ids on the same tick must not get
    // correlated streams, or a crowd wanders in formation.
    SmallRng::seed_from_u64(mix(uid.raw() ^ mix(tick)))
}

/// SplitMix64's finaliser: cheap, and good enough to decorrelate a counter.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A person.
///
/// No appearance field: [`GameEntity::appearance_seed`] already gives the
/// renderer a stable per-entity number, and storing a second one would be two
/// sources of truth for the same hairstyle.
pub struct Human {
    walk: Walker,
}

impl Human {
    pub fn new(uid: Uid, cell: Point) -> Human {
        Human {
            walk: Walker::new(uid, cell, HUMAN_SPEED),
        }
    }
}

impl GameEntity for Human {
    fn body(&self) -> &Body {
        &self.walk.body
    }

    fn body_mut(&mut self) -> &mut Body {
        &mut self.walk.body
    }

    fn think(&self, ctx: &Think<'_>) -> Intent {
        self.walk.think(ctx)
    }

    fn apply(&mut self, intent: &Intent) {
        self.walk.apply(intent);
    }
}

/// A dog.
pub struct Dog {
    walk: Walker,
    facing: Facing,
}

impl Dog {
    pub fn new(uid: Uid, cell: Point, facing: Facing) -> Dog {
        Dog {
            walk: Walker::new(uid, cell, DOG_SPEED),
            facing,
        }
    }

}

impl GameEntity for Dog {
    fn body(&self) -> &Body {
        &self.walk.body
    }

    fn body_mut(&mut self) -> &mut Body {
        &mut self.walk.body
    }

    fn think(&self, ctx: &Think<'_>) -> Intent {
        self.walk.think(ctx)
    }

    fn facing(&self) -> Option<Facing> {
        Some(self.facing)
    }

    /// Turns to face the way it is walking.
    ///
    /// Derived here rather than sent in the intent because it is a consequence
    /// of moving, not a decision: an intent describes what an entity wants,
    /// and no dog wants to face left.
    fn apply(&mut self, intent: &Intent) {
        let was = self.walk.body.position().0;
        self.walk.apply(intent);
        let now = self.walk.body.position().0;

        // Left alone when it did not move horizontally, so a dog walking
        // straight up does not flip to an arbitrary side.
        if now < was {
            self.facing = Facing::Left;
        } else if now > was {
            self.facing = Facing::Right;
        }
    }
}

/// Build an entity of a kind, at a cell.
///
/// The one place a `Uid`'s type tag and the concrete type behind it are tied
/// together, so they cannot drift apart.
pub(super) fn build(
    uid: Uid,
    kind: EntityType,
    cell: Point,
    rng: &mut SmallRng,
) -> Box<dyn GameEntity> {
    match kind {
        EntityType::Human => Box::new(Human::new(uid, cell)),
        EntityType::Dog => Box::new(Dog::new(
            uid,
            cell,
            if rng.random() { Facing::Left } else { Facing::Right },
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR, WALL};
    use crate::sim::log::Log;

    fn room() -> Map {
        Map::new(Size::new(9, 9), FLOOR)
    }

    fn ctx<'a>(map: &'a Map, log: &'a Log, tick: u64) -> Think<'a> {
        Think {
            map,
            log,
            dt: 1.0 / 60.0,
            tick,
        }
    }

    fn human(cell: Point) -> Human {
        Human::new(Uid::new(EntityType::Human, 42), cell)
    }

    #[test]
    fn a_wanderer_with_somewhere_to_go_moves_towards_it() {
        let (map, log) = (room(), Log::new());
        let mut walker = human(Point::new(4, 4));
        let start = walker.position();

        let intent = walker.think(&ctx(&map, &log, 1));
        walker.apply(&intent);

        assert_ne!(walker.position(), start);
        assert!(map.is_passable(walker.center_position()));
    }

    #[test]
    fn a_wanderer_boxed_in_by_walls_stands_still_rather_than_escaping() {
        let mut map = Map::new(Size::new(3, 3), WALL);
        // One passable cell, in the middle: nowhere to go.
        map.set_terrain(Point::new(1, 1), FLOOR);
        let log = Log::new();

        let mut walker = human(Point::new(1, 1));
        for tick in 0..50 {
            let intent = walker.think(&ctx(&map, &log, tick));
            walker.apply(&intent);
        }

        assert_eq!(walker.center_position(), Point::new(1, 1));
    }

    #[test]
    fn a_wanderer_never_ends_a_tick_inside_a_wall() {
        let mut map = room();
        // A wall down the middle column, so a goal on the far side is only
        // reachable by walking through it — which is what must not happen.
        for y in 0..9 {
            map.set_terrain(Point::new(4, y), WALL);
        }
        let log = Log::new();

        let mut walker = human(Point::new(1, 4));
        for tick in 0..600 {
            let intent = walker.think(&ctx(&map, &log, tick));
            walker.apply(&intent);
            assert!(
                map.is_passable(walker.center_position()),
                "walked into {:?} on tick {tick}",
                walker.center_position()
            );
        }
    }

    #[test]
    fn the_same_entity_on_the_same_tick_decides_the_same_thing() {
        let (map, log) = (room(), Log::new());
        let a = human(Point::new(4, 4));
        let b = human(Point::new(4, 4));

        for tick in 0..20 {
            let one = a.think(&ctx(&map, &log, tick));
            let two = b.think(&ctx(&map, &log, tick));
            assert_eq!(one, two, "tick {tick}");
        }
    }

    #[test]
    fn two_entities_on_the_same_tick_do_not_decide_in_lockstep() {
        // Neighbouring ids sharing a tick must not get correlated streams, or
        // a crowd wanders in formation.
        let (map, log) = (room(), Log::new());
        let one = Human::new(Uid::new(EntityType::Human, 1), Point::new(4, 4));
        let two = Human::new(Uid::new(EntityType::Human, 2), Point::new(4, 4));

        let differ = (0..20).any(|tick| {
            one.think(&ctx(&map, &log, tick)) != two.think(&ctx(&map, &log, tick))
        });
        assert!(differ);
    }

    #[test]
    fn a_dog_turns_to_face_the_way_it_walks() {
        let mut dog = Dog::new(Uid::new(EntityType::Dog, 7), Point::new(4, 4), Facing::Right);
        let here = dog.position();

        dog.apply(&Intent::Move {
            to: (here.0 - 1.0, here.1),
            goal: None,
        });
        assert_eq!(dog.facing(), Some(Facing::Left));

        dog.apply(&Intent::Move {
            to: (here.0 + 1.0, here.1),
            goal: None,
        });
        assert_eq!(dog.facing(), Some(Facing::Right));
    }

    #[test]
    fn a_dog_walking_straight_up_keeps_the_side_it_was_facing() {
        let mut dog = Dog::new(Uid::new(EntityType::Dog, 7), Point::new(4, 4), Facing::Left);
        let (x, y) = dog.position();
        dog.apply(&Intent::Move {
            to: (x, y + 1.0),
            goal: None,
        });
        assert_eq!(dog.facing(), Some(Facing::Left));
    }

    #[test]
    fn an_idle_intent_moves_nothing() {
        let mut walker = human(Point::new(4, 4));
        let before = walker.position();
        walker.apply(&Intent::Idle);
        assert_eq!(walker.position(), before);
    }
}
