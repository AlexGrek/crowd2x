//! [`Walker`]: the movement action — find a route to a cell, and walk it.
//!
//! It used to be the whole of a wanderer: pick a cell, route to it, walk,
//! pick another. Picking is a decision, and decisions belong to the brain now
//! (`brain::goals::wander`); what is left here is the part that was always
//! movement, and it is all still here — two-stage A\*, detour-on-block, corner
//! cutting, [`MAX_STEP`]. A walker is told where to go and says whether it got
//! there. It never decides where.
//!
//! # Two stages, and the difference between them is what they can see
//!
//! * **Far** — [`Walker::route_to`], point to point over the *static*
//!   passability map. It does not know the crowd exists. The terrain does not
//!   move, so a route over the terrain stays true for as long as the walker
//!   takes to walk it, and nothing this stage reads changes while the round
//!   runs — which is what lets every walker in the world plan at the same
//!   time, on every thread there is.
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
//! the crowd is what a wanderer once did, and it meant a doorway with one
//! person in it was a wall.
//!
//! Neither stage searches diagonals ([`super::path`] says why), but **movement
//! is not orthogonal**: a walker picks up the next heading as it crosses into
//! a cell rather than at the middle of it, so it cuts corners.
//!
//! # Where the searching happens
//!
//! [`Walker::think`] does arithmetic and nothing else — it walks the route it
//! was given. Both stages of planning run in the reaction round, driven by the
//! brain, for two reasons that happen to agree: a route is state, and think
//! may not write; and the crowd is only worth planning against once the whole
//! crowd has moved.

use crate::map::Point;

use super::brain::ActionState;
use super::entity::{Body, Think};
use super::path::{self, Path};
use super::uid::Uid;
use super::{Intent, MoveOutcome};

/// The far stage's budget, in cells expanded. Deliberately large: a walker
/// crossing a real map should be limited by the map and not by this, and the
/// number is here to bound the *unreachable* case rather than the long one.
/// See [`path::PathFinder::find`].
pub const FAR_LIMIT: usize = 65_536;

/// How many cells of the plan the near stage throws away and replaces.
///
/// Five is far enough ahead to walk round somebody standing in a doorway and
/// short enough that the detour is a local repair rather than a second full
/// path — the far route past those five cells is still good, because whatever
/// is in the way is a body and bodies move.
pub const DETOUR_CELLS: usize = 5;

/// The near stage's budget, in cells expanded. Small on purpose: this runs on
/// a collision, collisions come in crowds, and a detour that has to search
/// half the map is not a detour. Failing here fails the walk, and whoever
/// asked for it decides what that means.
pub const DETOUR_LIMIT: usize = 512;

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
pub const MAX_STEP: f32 = 0.5;

/// The part of an entity that walks: shared by every kind, because "has a
/// position, a route, and somewhere that route ends" is not specific to any of
/// them.
///
/// Not `Copy`, and that is the [`Path`]: a route is a `Vec`, and one per
/// walker is the cost of the whole feature. It is paid once per route and not
/// per tick — walking a path allocates nothing, and neither does being blocked
/// on one that a detour happens to fit inside.
#[derive(Clone, Debug)]
pub struct Walker {
    body: Body,
    /// How to get where it is going, cell by cell, orthogonally. Empty means
    /// it is not going anywhere.
    path: Path,
    /// Cells per second.
    speed: f32,
}

#[cfg(test)]
thread_local! {
    /// How many far routes this thread has asked for — the counting half of
    /// `brain`'s test that a brain never asks for a route more than once a
    /// task. Per thread, so tests running side by side do not count each
    /// other's.
    pub(crate) static ROUTES_ASKED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

impl Walker {
    pub fn new(uid: Uid, cell: Point, speed: f32) -> Walker {
        Walker {
            body: Body::at_cell(uid, cell),
            path: Path::none(),
            speed,
        }
    }

    pub fn body(&self) -> &Body {
        &self.body
    }

    pub fn body_mut(&mut self) -> &mut Body {
        &mut self.body
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// **Walk the plan.** Aim at the centre of the next cell in the path, and
    /// nothing else.
    ///
    /// There is no decision left in here: the route was chosen in the reaction
    /// round, which is where the `&mut self` a route has to be stored through
    /// lives. What remains is arithmetic, which is exactly the shape the read
    /// phase wants — no RNG, no search, no branch on the crowd.
    pub fn think(&self, ctx: &Think<'_>) -> Intent {
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
    ///
    /// **Except the last cell**, which is left for [`Walker::advance`] to tick
    /// off once the body stands in the middle of it. There is no corner after
    /// it to cut, and a walk that ended on entering would leave whoever walked
    /// it standing on the line between two cells — eating at a fridge from
    /// the edge of the tile beside it.
    pub fn apply(&mut self, intent: &Intent) {
        let Intent::Move { to } = intent else {
            return;
        };
        self.body.set_position(*to);
        if self.path.remaining().len() > 1 && self.path.current() == Some(self.body.center_position())
        {
            self.path.advance();
        }
    }

    /// Whether the body is in the middle of the cell it is standing in.
    fn is_centred(&self) -> bool {
        let (x, y) = self.body.position();
        let cell = self.body.center_position();
        let (dx, dy) = (cell.x as f32 + 0.5 - x, cell.y as f32 + 0.5 - y);
        (dx * dx + dy * dy).sqrt() <= ARRIVED
    }

    /// **The far stage.** Find the way to `cell` over the static passability
    /// map and nothing else, and make it the route. Returns whether there was
    /// one.
    ///
    /// The crowd is deliberately not consulted. A route planned round every
    /// body standing in the way at this instant is a route that is wrong by
    /// the time it is walked, and it would make every walker's plan depend on
    /// every other walker's position. The terrain does not move, so a route
    /// over the terrain stays true, and bodies are the near stage's problem.
    ///
    /// A miss clears the route and says so. What to do about it — try
    /// somewhere else, hold off, give up on the whole idea — is a decision,
    /// and belongs to whoever asked.
    pub fn route_to(&mut self, ctx: &Think<'_>, cell: Point) -> bool {
        #[cfg(test)]
        ROUTES_ASKED.with(|asked| asked.set(asked.get() + 1));

        let here = self.body.center_position();
        match path::find_path(here, cell, FAR_LIMIT, |c| ctx.is_passable(c)) {
            // Already in the cell, which still means walking to the middle of
            // it: a route of one step, ticked off by `advance` on arrival.
            Some(steps) if steps.is_empty() => {
                self.path = Path::new(vec![cell]);
                true
            }
            Some(steps) => {
                self.path = Path::new(steps);
                true
            }
            None => {
                self.path.clear();
                false
            }
        }
    }

    /// Stop walking: drop the route, wherever it was going.
    pub fn halt(&mut self) {
        self.path.clear();
    }

    /// **Step directly onto `cell`, no search.** For the one case a route
    /// would refuse to reach: a cell that is impassable to ordinary movement
    /// but enterable by whoever holds its claim
    /// ([`crate::sim::feature::Access::Entered`]). [`Walker::route_to`]'s
    /// predicate would refuse the very cell this exists to reach, and
    /// searching for it would be pointless anyway — by the time this is
    /// called the cell is always exactly one step away, so there is nothing
    /// to plan. The move step still has the final say: this only sets the
    /// route, the way [`Walker::route_to`] does, and a claim already held by
    /// somebody else refuses the step exactly as a wall would.
    pub fn enter(&mut self, cell: Point) {
        self.path = Path::new(vec![cell]);
    }

    /// **The reaction round for a walk in progress.** Repair the route if the
    /// world refused this tick's step, and say where the walk has got to.
    ///
    /// * blocked → [`Walker::detour`]; a detour that finds nothing is
    ///   [`ActionState::Failed`], with the route cleared
    /// * route walked, standing in the middle of its last cell →
    ///   [`ActionState::Finished`]
    /// * otherwise → [`ActionState::Running`]
    pub fn advance(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) -> ActionState {
        if outcome.is_blocked() && !self.detour(ctx) {
            return ActionState::Failed;
        }
        // The last step, which `apply` leaves for arrival rather than entry.
        if self.path.remaining().len() == 1
            && self.path.current() == Some(self.body.center_position())
            && self.is_centred()
        {
            self.path.advance();
        }
        if self.path.is_done() {
            ActionState::Finished
        } else {
            ActionState::Running
        }
    }

    /// **The near stage.** Something was in the way: throw away the next few
    /// cells of the plan and find another way to where they ended. Returns
    /// `false`, with the route cleared, when there is no local way round.
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
    pub fn detour(&mut self, ctx: &Think<'_>) -> bool {
        let remaining = self.path.remaining();
        if remaining.is_empty() {
            return true;
        }
        let cells = remaining.len().min(DETOUR_CELLS);
        let target = remaining[cells - 1];

        let uid = self.body.uid();
        let here = self.body.center_position();
        let detour = path::find_path(here, target, DETOUR_LIMIT, |cell| {
            ctx.is_passable(cell) && (cell == target || ctx.occupancy.is_free_for(cell, uid))
        });

        match detour {
            Some(detour) => {
                self.path.splice(cells, detour);
                true
            }
            None => {
                self.path.clear();
                false
            }
        }
    }

    /// Where the route ends, if there is one.
    pub fn destination(&self) -> Option<Point> {
        if self.path.is_done() {
            None
        } else {
            self.path.destination()
        }
    }

    /// Whether there is any route left to walk.
    pub fn is_walking(&self) -> bool {
        !self.path.is_done()
    }

    /// What a walker would tell a debugger: where it is going, how far it has
    /// left to walk to get there, and how fast.
    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "dest",
                match self.destination() {
                    Some(cell) => format!("{}, {}", cell.x, cell.y),
                    None => "none".to_string(),
                },
            ),
            ("path", format!("{} cells", self.path.remaining().len())),
            ("speed", format!("{:.2} cells/s", self.speed)),
        ]
    }

    /// The route still to walk, current step first — see
    /// [`super::GameEntity::planned_path`].
    pub fn path_cells(&self) -> Vec<Point> {
        self.path.remaining().to_vec()
    }

    #[cfg(test)]
    pub(crate) fn set_path(&mut self, steps: Vec<Point>) {
        self.path = Path::new(steps);
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR, WALL};
    use crate::sim::entity::cell_of;
    use crate::sim::feature::Features;
    use crate::sim::log::Log;
    use crate::sim::occupancy::Occupancy;
    use crate::sim::uid::EntityType;

    struct World {
        map: Map,
        occupancy: Occupancy,
        log: Log,
        features: Features,
    }

    impl World {
        fn new(map: Map) -> World {
            World {
                occupancy: Occupancy::new(map.size()),
                map,
                log: Log::new(),
                features: Features::default(),
            }
        }

        fn ctx(&self, tick: u64) -> Think<'_> {
            Think {
                map: &self.map,
                occupancy: &self.occupancy,
                log: &self.log,
                features: &self.features,
                dt: 1.0 / 60.0,
                tick,
            }
        }
    }

    fn walker(cell: Point) -> Walker {
        Walker::new(Uid::new(EntityType::Human, 42), cell, 2.0)
    }

    #[test]
    fn a_walker_with_no_route_thinks_idle() {
        let world = World::new(Map::new(Size::new(9, 9), FLOOR));
        assert_eq!(walker(Point::new(4, 4)).think(&world.ctx(0)), Intent::Idle);
    }

    #[test]
    fn an_idle_intent_moves_nothing() {
        let mut walker = walker(Point::new(4, 4));
        let before = walker.body().position();
        walker.apply(&Intent::Idle);
        assert_eq!(walker.body().position(), before);
    }

    #[test]
    fn a_walker_walks_a_route_it_was_given_to_the_end_and_says_so() {
        let world = World::new(Map::new(Size::new(9, 9), FLOOR));
        let mut walker = walker(Point::new(1, 1));
        let goal = Point::new(6, 3);
        assert!(walker.route_to(&world.ctx(0), goal));

        let mut finished = false;
        for t in 0..600 {
            let ctx = world.ctx(t);
            let intent = walker.think(&ctx);
            walker.apply(&intent);
            match walker.advance(&ctx, MoveOutcome::Moved) {
                ActionState::Running => {}
                ActionState::Finished => {
                    finished = true;
                    break;
                }
                ActionState::Failed => panic!("nothing was in the way"),
            }
        }
        assert!(finished);
        assert_eq!(walker.body().center_position(), goal);
    }

    #[test]
    fn a_walk_ends_in_the_middle_of_its_last_cell() {
        // Entering the last cell is not arriving: whoever walked there is about
        // to stand still in it, and standing on a cell's edge reads as being
        // between two places.
        let world = World::new(Map::new(Size::new(9, 9), FLOOR));
        let mut walker = walker(Point::new(1, 1));
        assert!(walker.route_to(&world.ctx(0), Point::new(5, 1)));

        for t in 0..600 {
            let ctx = world.ctx(t);
            let intent = walker.think(&ctx);
            walker.apply(&intent);
            if walker.advance(&ctx, MoveOutcome::Moved) == ActionState::Finished {
                break;
            }
        }
        let (x, y) = walker.body().position();
        assert!((x - 5.5).abs() < 1e-3 && (y - 1.5).abs() < 1e-3, "stopped at {x}, {y}");
    }

    #[test]
    fn a_walk_to_the_cell_it_is_already_in_still_centres_it() {
        let world = World::new(Map::new(Size::new(9, 9), FLOOR));
        let mut walker = walker(Point::new(3, 3));
        walker.body_mut().set_position((3.02, 3.9));
        assert!(walker.route_to(&world.ctx(0), Point::new(3, 3)));
        assert!(walker.is_walking(), "off-centre is somewhere left to walk");

        let mut finished = false;
        for t in 0..600 {
            let ctx = world.ctx(t);
            let intent = walker.think(&ctx);
            walker.apply(&intent);
            if walker.advance(&ctx, MoveOutcome::Moved) == ActionState::Finished {
                finished = true;
                break;
            }
        }
        assert!(finished);
        let (x, y) = walker.body().position();
        assert!((x - 3.5).abs() < 1e-3 && (y - 3.5).abs() < 1e-3, "stopped at {x}, {y}");
    }

    #[test]
    fn entering_sets_a_one_cell_route_with_no_search() {
        // No terrain on this map would let a route reach (4, 4) at all —
        // `enter` has to not care, because it never asks.
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        map.set_terrain(Point::new(4, 4), WALL);
        let world = World::new(map);
        let mut walker = walker(Point::new(3, 4));
        let target = Point::new(4, 4);

        walker.enter(target);
        assert_eq!(walker.destination(), Some(target));
        assert!(walker.is_walking());

        for t in 0..600 {
            let ctx = world.ctx(t);
            let intent = walker.think(&ctx);
            walker.apply(&intent);
            if walker.advance(&ctx, MoveOutcome::Moved) == ActionState::Finished {
                break;
            }
        }
        assert_eq!(walker.body().center_position(), target);
    }

    #[test]
    fn an_ordinary_route_treats_an_enterable_feature_exactly_like_a_wall() {
        // A toilet plugging the only doorway through: an `Access::Entered`
        // cell is impassable for `route_to` and `detour` alike, the far
        // stage's predicate never having heard of `Features::is_enterable`.
        // Whatever `Action::enter` lets one task do, an ordinary walk is not
        // it — that is the whole of "still impassable for coming through
        // it".
        let mut map = Map::new(Size::new(9, 5), FLOOR);
        for x in 0..9 {
            map.set_terrain(Point::new(x, 2), WALL);
        }
        let toilet = Point::new(4, 2);
        map.set_terrain(toilet, FLOOR);
        crate::sim::testing::prop_at(&mut map, "toilet", toilet);
        let features = Features::from_map(&map);
        assert!(features.is_enterable(toilet), "the fixture should have made a real toilet");

        let mut world = World::new(map);
        world.features = features;
        let mut walker = walker(Point::new(1, 1));

        assert!(!walker.route_to(&world.ctx(0), Point::new(7, 4)), "the toilet is not a doorway");
        assert!(!walker.is_walking());
    }

    #[test]
    fn a_route_to_somewhere_walled_off_says_so_and_leaves_no_plan() {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        for y in 0..9 {
            map.set_terrain(Point::new(4, y), WALL);
        }
        let world = World::new(map);
        let mut walker = walker(Point::new(1, 1));
        walker.set_path(vec![Point::new(1, 2)]);

        assert!(!walker.route_to(&world.ctx(0), Point::new(7, 1)));
        assert!(!walker.is_walking(), "a stale route must not survive a miss");
    }

    #[test]
    fn a_walker_that_reaches_a_wall_by_hand_gets_stopped_by_the_move_step() {
        // The far stage routes round a wall on its own, so the only way to see
        // a move step refuse one here is to hand a walker a path that walks
        // into it directly — which is what
        // `sim::mod::nothing_ever_ends_a_tick_in_an_impassable_cell` checks
        // for real, through the move step and not by hand.
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        map.set_terrain(Point::new(4, 4), WALL);
        let world = World::new(map);

        let mut walker = walker(Point::new(3, 4));
        walker.set_path(vec![Point::new(4, 4)]);

        // `think` only ever asks for a step this small (`MAX_STEP`), so
        // reaching the wall takes several of them — apply each one by hand
        // and stop as soon as an aim lands outside passable terrain.
        let mut aimed_at_the_wall = false;
        for t in 0..600 {
            let intent = walker.think(&world.ctx(t));
            if matches!(intent, Intent::Move { to } if !world.map.is_passable(cell_of(to))) {
                aimed_at_the_wall = true;
                break;
            }
            walker.apply(&intent);
        }
        assert!(aimed_at_the_wall, "never even tried the wall");
    }

    #[test]
    fn a_blocked_walker_detours_round_whoever_is_in_the_way() {
        // A straight corridor with a body parked one cell ahead: the near
        // stage should find the way round it and keep heading for the same
        // place, rather than dropping the route the far stage already paid for.
        let mut world = World::new(Map::new(Size::new(9, 9), FLOOR));
        let blocker = Uid::new(EntityType::Human, 99);
        world.occupancy.claim(Point::new(5, 4), blocker).unwrap();

        let mut walker = walker(Point::new(4, 4));
        let destination = Point::new(7, 4);
        walker.set_path(vec![Point::new(5, 4), Point::new(6, 4), destination]);

        let state = walker.advance(&world.ctx(0), MoveOutcome::Blocked { by: Some(blocker) });

        assert_eq!(state, ActionState::Running);
        assert_eq!(walker.destination(), Some(destination));
        assert!(
            !walker.path().remaining().contains(&Point::new(5, 4)),
            "should not still be heading straight at the blocker"
        );
    }

    #[test]
    fn a_detour_that_finds_no_way_round_fails_the_walk() {
        // A corridor one cell wide with somebody standing in it: there is no
        // local way round, and saying so is the walker's whole answer.
        let mut map = Map::new(Size::new(9, 1), FLOOR);
        map.set_terrain(Point::new(8, 0), FLOOR);
        let mut world = World::new(map);
        let blocker = Uid::new(EntityType::Human, 99);
        world.occupancy.claim(Point::new(2, 0), blocker).unwrap();

        let mut walker = walker(Point::new(1, 0));
        walker.set_path((2..=6).map(|x| Point::new(x, 0)).collect());

        let state = walker.advance(&world.ctx(0), MoveOutcome::Blocked { by: Some(blocker) });
        assert_eq!(state, ActionState::Failed);
        assert!(!walker.is_walking());
    }

    #[test]
    fn the_same_walker_asked_the_same_thing_plans_the_same_route() {
        let world = World::new(Map::new(Size::new(16, 16), FLOOR));
        let (mut a, mut b) = (walker(Point::new(2, 3)), walker(Point::new(2, 3)));
        a.route_to(&world.ctx(5), Point::new(13, 11));
        b.route_to(&world.ctx(5), Point::new(13, 11));
        assert_eq!(a.path(), b.path());
    }
}
