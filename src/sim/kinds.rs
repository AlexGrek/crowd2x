//! The entities that exist: humans and dogs.
//!
//! Both are wanderers for now — pick a cell somewhere in the neighbourhood,
//! **find a route to it**, walk the route, pick another. That is not the
//! simulation, it is the smallest thing that exercises every part of the tick:
//! thinking against the map, an intent crossing from the read phase to the
//! write phase, a move the world is allowed to refuse, and a reaction to the
//! refusal.
//!
//! # Two stages, and the difference between them is what they can see
//!
//! A wanderer no longer walks at its goal in a straight line and give up when
//! a wall turns out to be in the way. It plans, and it plans twice:
//!
//! * **Far** — [`Walker::plan`], point to point over the *static* passability
//!   map. It does not know the crowd exists. The terrain does not move, so a
//!   route over the terrain stays true for as long as the walker takes to walk
//!   it, and nothing this stage reads changes while the round runs — which is
//!   what lets every walker in the world plan at the same time, on every
//!   thread there is.
//! * **Near** — [`Walker::detour`], on a move the world refused. The next
//!   [`DETOUR_CELLS`] cells of the plan are thrown away and re-planned against
//!   passability **as it is at this moment**, crowd included, ending at the
//!   same cell they ended at. The far route past that point is untouched,
//!   because it is still right: whatever was in the way is a body, and bodies
//!   move.
//!
//! The split is the point. Planning the whole route against the crowd would
//! make every walker's plan depend on every other walker's position — the
//! per-agent scan over every other agent that killed the earlier prototype —
//! and it would be wrong by the time it was walked. Planning nothing against
//! the crowd is what a wanderer used to do, and it meant a doorway with one
//! person in it was a wall.
//!
//! Neither stage searches diagonals ([`super::path`] says why), but **movement
//! is not orthogonal**: a walker picks up the next heading as it crosses into
//! a cell rather than at the middle of it, so it cuts corners.
//!
//! # Where the decisions live
//!
//! [`Walker::think`] does arithmetic and nothing else — it walks the plan it
//! was given. Both stages of planning are in [`Walker::react`], for two
//! reasons that happen to agree: a route is state, and think may not write;
//! and the crowd is only worth planning against once the whole crowd has
//! moved.
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
use super::{Intent, MoveOutcome};

use super::path::{self, Path};

/// How far a wanderer will look for somewhere to go, in cells.
///
/// Wider than it was when a wanderer walked in a straight line at its goal,
/// because it no longer has to: a route round a wall is a route, so a cell
/// across the room is as reachable as the one next door and picking only from
/// next door would waste the search.
const WANDER_RADIUS: i32 = 16;

/// How many cells to try before giving up and standing still this tick.
const WANDER_TRIES: u32 = 8;

/// The far stage's budget, in cells expanded. Deliberately large: a wanderer
/// crossing a real map should be limited by the map and not by this, and the
/// number is here to bound the *unreachable* case rather than the long one.
/// See [`path::PathFinder::find`].
const FAR_LIMIT: usize = 65_536;

/// How many cells of the plan the near stage throws away and replaces.
///
/// Five is far enough ahead to walk round somebody standing in a doorway and
/// short enough that the detour is a local repair rather than a second full
/// path — the far route past those five cells is still good, because whatever
/// is in the way is a body and bodies move.
const DETOUR_CELLS: usize = 5;

/// The near stage's budget, in cells expanded. Small on purpose: this runs on
/// a collision, collisions come in crowds, and a detour that has to search
/// half the map is not a detour. Failing here drops the whole path and the
/// far stage plans again next tick, which is the right answer anyway.
const DETOUR_LIMIT: usize = 512;

/// Ticks to wait before planning again, after a plan that found nothing.
///
/// A walker with nowhere reachable to go would otherwise pay for a search
/// every tick forever, and the search it pays for is the expensive one: a goal
/// it cannot reach makes A* flood everything it can. Roughly half a second, so
/// a door opening is noticed promptly and a sealed room costs almost nothing.
const REPLAN_DELAY: u16 = 32;

/// Close enough to a cell's centre to be treated as standing on it, in cells.
/// Only a guard against dividing by a zero distance — arriving is a matter of
/// which cell the walker is in, not how near the middle of it it got.
const ARRIVED: f32 = 1e-4;

/// The furthest anything moves in one tick, in cells.
///
/// The move step checks the *destination* cell, so a step longer than a cell
/// would step straight over a wall and land legally on the far side — the
/// check would pass and the wall would not exist. Capping below one cell means
/// a step crosses at most one boundary per axis, which is what makes the
/// destination check sufficient.
///
/// At the fixed timestep nothing comes close to this (a dog covers 0.055 cells
/// a tick). It is here because `process_game_state` is public and takes
/// whatever `dt` it is handed, and "only correct at one timestep" is not a
/// property worth relying on.
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
/// position, a route, and somewhere that route ends" is not specific to any of
/// them.
///
/// Not `Copy` any more, and that is the [`Path`]: a route is a `Vec`, and one
/// per walker is the cost of the whole feature. It is paid once per plan and
/// not per tick — walking a path allocates nothing, and neither does being
/// blocked on one that a detour happens to fit inside.
#[derive(Clone, Debug)]
pub struct Walker {
    body: Body,
    /// Where it is heading, in cells. `None` means "no plan": the far stage
    /// picks one in the reaction round.
    goal: Option<Point>,
    /// How to get there, cell by cell, orthogonally. Empty means the same
    /// thing `goal: None` does, and the two are cleared together.
    path: Path,
    /// Cells per second.
    speed: f32,
    /// Ticks left before the far stage may try again. Non-zero only after a
    /// plan that found nothing — see [`REPLAN_DELAY`].
    delay: u16,
}

impl Walker {
    fn new(uid: Uid, cell: Point, speed: f32) -> Walker {
        Walker {
            body: Body::at_cell(uid, cell),
            goal: None,
            path: Path::none(),
            speed,
            delay: 0,
        }
    }

    /// **Walk the plan.** Aim at the centre of the next cell in the path, and
    /// nothing else.
    ///
    /// There is no decision left in here: the route was chosen in the reaction
    /// round, by [`Walker::plan`] and [`Walker::detour`], which is where the
    /// `&mut self` a route has to be stored through lives. What remains is
    /// arithmetic, which is exactly the shape the read phase wants — no RNG,
    /// no search, no branch on the crowd.
    ///
    /// A walker with no path stands still for this tick and gets one at the
    /// end of it. That is a single tick at spawn and never again.
    fn think(&self, ctx: &Think<'_>) -> Intent {
        let Some(step) = self.path.current() else {
            return Intent::Idle;
        };

        let (x, y) = self.body.position();
        let (tx, ty) = (step.x as f32 + 0.5, step.y as f32 + 0.5);
        let (dx, dy) = (tx - x, ty - y);
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= ARRIVED {
            return Intent::Idle;
        }

        let step = (self.speed * ctx.dt).min(distance).min(MAX_STEP);
        Intent::Move {
            to: (x + dx / distance * step, y + dy / distance * step),
        }
    }

    /// Take the step the world granted, and tick the path along if it landed
    /// in the cell it was heading for.
    ///
    /// Advancing on **entering** the cell rather than on reaching its centre
    /// is what makes a corner read as a turn instead of a stop: the next
    /// heading is picked up at the boundary, so the walked line cuts the
    /// corner diagonally. `path.rs` says why that can never skip a cell.
    fn apply(&mut self, intent: &Intent) {
        let Intent::Move { to } = intent else {
            return;
        };
        self.body.set_position(*to);
        if self.path.current() == Some(self.body.center_position()) {
            self.path.advance();
        }
    }

    /// **The reaction round: repair the plan, or make one.**
    ///
    /// Both stages of pathfinding are here, and they are here rather than in
    /// [`Walker::think`] for two separate reasons that happen to agree:
    ///
    /// * A route is state, and think may not write. React is the round that
    ///   may write to `self` and only to `self`.
    /// * The near stage plans against the crowd, and the only moment the crowd
    ///   is worth planning against is *after* everyone has moved. Deciding a
    ///   detour inside the move loop would route round entities that have not
    ///   taken their step yet.
    ///
    /// The two run in that order on purpose: a detour that fails clears the
    /// path, and a cleared path is what the far stage plans for, so a walker
    /// that is thoroughly stuck recovers in one tick rather than two.
    fn react(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) {
        if outcome.is_blocked() {
            self.detour(ctx);
        }
        if self.path.is_done() {
            self.plan(ctx);
        }
    }

    /// **The near stage.** Something was in the way: throw away the next few
    /// cells of the plan and find another way to where they ended.
    ///
    /// [`DETOUR_CELLS`] of the route go, or all of what is left if it is
    /// shorter, and the last of them is the target — so the rest of the far
    /// route past that point survives untouched. It is the crowd this is
    /// planned against as well as the terrain, which is the whole difference
    /// from the far stage and the reason it can find a way round a body the
    /// far stage cannot see.
    ///
    /// The target cell itself is allowed to be occupied. Refusing it would
    /// make a walker whose next-but-four cell happens to have somebody in it
    /// throw away a good route; letting it stand means the walker gets there,
    /// is blocked, and detours again — by which time whoever it was has
    /// probably moved.
    ///
    /// No detour means no local way round, so the plan goes and the far stage
    /// takes over. That is the honest answer: waiting for a gap needs a reason
    /// to believe one is coming, and a wanderer has nowhere it needs to be.
    fn detour(&mut self, ctx: &Think<'_>) {
        let remaining = self.path.remaining();
        if remaining.is_empty() {
            return;
        }
        let cells = remaining.len().min(DETOUR_CELLS);
        let target = remaining[cells - 1];

        let uid = self.body.uid();
        let here = self.body.center_position();
        let detour = path::find_path(here, target, DETOUR_LIMIT, |cell| {
            ctx.is_passable(cell) && (cell == target || ctx.occupancy.is_free_for(cell, uid))
        });

        match detour {
            Some(detour) => self.path.splice(cells, detour),
            None => self.give_up(),
        }
    }

    /// **The far stage.** Pick somewhere to go and find the way there, over
    /// the static passability map and nothing else.
    ///
    /// The crowd is deliberately not consulted. A route planned round every
    /// body standing in the way at this instant is a route that is wrong by
    /// the time it is walked, and it would make every walker's plan depend on
    /// every other walker's position — which is the per-agent scan over every
    /// other agent that killed the earlier prototype. The terrain does not
    /// move, so a route over the terrain stays true, and bodies are the near
    /// stage's problem.
    ///
    /// That is also what makes this the parallel half: nothing it reads
    /// changes while the round runs.
    fn plan(&mut self, ctx: &Think<'_>) {
        if let Some(left) = self.delay.checked_sub(1) {
            self.delay = left;
            return;
        }

        let here = self.body.center_position();
        let mut rng = tick_rng(self.body.uid(), ctx.tick);
        let Some(goal) = self.pick_goal(ctx, &mut rng) else {
            // Walled in, or standing on a map with nothing walkable.
            self.give_up();
            self.report_stuck(ctx);
            return;
        };

        match path::find_path(here, goal, FAR_LIMIT, |cell| ctx.is_passable(cell)) {
            Some(steps) if !steps.is_empty() => {
                self.goal = Some(goal);
                self.path = Path::new(steps);
            }
            // Passable but not reachable — a room on the far side of a wall.
            // Same answer as nowhere to go: wait a moment and pick again.
            _ => {
                self.give_up();
                self.report_stuck(ctx);
            }
        }
    }

    /// A passable cell within [`WANDER_RADIUS`], or `None` if a few tries
    /// found nothing.
    ///
    /// Bounded tries rather than a scan: this runs per idle entity per tick,
    /// and a bounded miss costs one wasted tick while a scan of the
    /// neighbourhood would cost the frame. Reachability is not checked here
    /// and cannot cheaply be — that is what the search that follows is for.
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

    /// Drop the plan and wait [`REPLAN_DELAY`] ticks before making another.
    ///
    /// The delay is the whole point: without it a walker with nowhere to go
    /// pays for a full flood of everything it can reach, every tick, forever,
    /// and a crowd of them is a frame-rate cliff that only appears on maps
    /// with a sealed room in them.
    fn give_up(&mut self) {
        self.goal = None;
        self.path.clear();
        self.delay = REPLAN_DELAY;
    }

    /// Say so when there is nowhere to go, at most once a second.
    ///
    /// Rate-limited on the tick rather than on a stored counter so that a
    /// crowd of stuck walkers does not turn the log into one line each per
    /// tick, which is how a log becomes noise.
    fn report_stuck(&self, ctx: &Think<'_>) {
        if ctx.tick.is_multiple_of(STUCK_LOG_TICKS) {
            ctx.log
                .push(format!("{} has nowhere to go", self.body.uid()));
        }
    }
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

    fn react(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) {
        self.walk.react(ctx, outcome);
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
    fn react(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) {
        self.walk.react(ctx, outcome);
    }

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
    use crate::sim::entity::cell_of;
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

    /// A wanderer aims at a wall quite happily — it is the move step that says
    /// no, which is why this asserts on the aim and not on the position.
    ///
    /// Nobody ending a tick inside a wall is a property of the whole tick, and
    /// it is tested where it is enforced:
    /// `sim::tests::nothing_ever_ends_a_tick_in_an_impassable_cell`. Applying
    /// an intent by hand, as this file's tests do, is deliberately taking the
    /// world's arbitration out of the loop.
    #[test]
    fn a_wanderer_walks_into_a_wall_and_leaves_the_refusing_to_the_move_step() {
        let mut map = room();
        for y in 0..9 {
            map.set_terrain(Point::new(4, y), WALL);
        }
        let log = Log::new();

        // Standing next to the wall with the only goal it can pick beyond it:
        // a 3x3 world would do, but the wall has to be reachable in one step.
        let mut walker = human(Point::new(3, 4));
        let mut aimed_at_the_wall = false;
        for tick in 0..600 {
            let intent = walker.think(&ctx(&map, &log, tick));
            if matches!(intent, Intent::Move { to, .. } if !map.is_passable(cell_of(to))) {
                aimed_at_the_wall = true;
                break;
            }
            walker.apply(&intent);
        }
        assert!(aimed_at_the_wall, "never even tried the wall");
    }

    #[test]
    fn a_blocked_walker_gives_up_the_goal_it_could_not_reach() {
        // The reaction round, on its own: whatever was in the way, the plan
        // that ran into it is dropped, and the next think picks another.
        let (map, log) = (room(), Log::new());
        let mut walker = human(Point::new(4, 4));

        let intent = walker.think(&ctx(&map, &log, 1));
        walker.apply(&intent);
        assert!(walker.walk.goal.is_some(), "it should be heading somewhere");

        walker.react(&ctx(&map, &log, 1), MoveOutcome::Blocked { by: None });
        assert_eq!(walker.walk.goal, None);
    }

    #[test]
    fn a_walker_that_got_where_it_was_going_keeps_its_plan() {
        let (map, log) = (room(), Log::new());
        let mut walker = human(Point::new(4, 4));

        let intent = walker.think(&ctx(&map, &log, 1));
        walker.apply(&intent);
        let goal = walker.walk.goal;

        walker.react(&ctx(&map, &log, 1), MoveOutcome::Moved);
        assert_eq!(walker.walk.goal, goal);
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
