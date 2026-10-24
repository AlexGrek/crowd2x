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
//!   passability map, the cheapest way by the terrain's walk costs — along
//!   the pavement and over the road, not down it. It does not know the crowd
//!   exists. The terrain does not
//!   move, so a route over the terrain stays true for as long as the walker
//!   takes to walk it, and nothing this stage reads changes while the round
//!   runs — which is what lets every walker in the world plan at the same
//!   time, on every thread there is.
//! * **Near** — [`Walker::detour`], on a move the world refused. The next
//!   [`DETOUR_CELLS`] cells of the plan are thrown away and re-planned against
//!   passability **as it is at this moment**, crowd included and by the same
//!   costs — a detour steps onto the road only when it has to — ending at the
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
//! # How fast
//!
//! A walker has a pace of its own, [`Walker::speed`] — a kind's speed, and
//! for a person a little more or less ([`kinds`](super::kinds) rolls it) —
//! and goes faster than that when it is in a hurry. **How much of a hurry is
//! the brain's to say** ([`Walker::hurry`], told the urgency of the goal in
//! charge every tick); how fast that gets it there is the walker's: it eases
//! towards the hurry it is asked for over [`SECONDS_TO_HURRY`], from a
//! standstill on every walk, so a unit breaks into a run rather than jumping
//! to one. Flat out is [`HURRY`] over its own pace.
//!
//! # Where the searching happens
//!
//! [`Walker::think`] does arithmetic and nothing else — it walks the route it
//! was given. Both stages of planning run in the reaction round, driven by the
//! brain, for two reasons that happen to agree: a route is state, and think
//! may not write; and the crowd is only worth planning against once the whole
//! crowd has moved.

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::brain::ActionState;
use super::entity::{Body, Think};
use super::path::{self, Allowance, Destinations, Path, PathFinder, SearchStatus};
use super::uid::Uid;
use super::{Intent, MoveOutcome};

/// The most one walker's far stage expands in one tick, in cells.
/// Deliberately large: a walker crossing a real map should be limited by the
/// map and not by this, and the number bounds one tick of work; unfinished
/// static searches resume next tick. See [`path::PathFinder::advance`]. In a
/// world, what the whole crowd may expand in a tick is bounded too, by the
/// route budget the react step shares out (`sim::ROUTE_BUDGET`), and a walker
/// gets the lesser of the two.
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

/// How much faster than its own pace a walker goes flat out: three quarters
/// again, a person's walk become a run (and as fast as a dog trots).
pub const HURRY: f32 = 0.75;

/// Watched seconds it takes a walker to go from its own pace to flat out,
/// and from flat out back to its pace: half a dozen cells of a person
/// breaking into a run. Watched, not world time — this is something seen.
pub const SECONDS_TO_HURRY: f32 = 1.0;

/// The part of an entity that walks: shared by every kind, because "has a
/// position, a route, and somewhere that route ends" is not specific to any of
/// them.
///
/// Route storage is boxed once at spawn. Its far and repair buffers retain
/// capacity across walks. Only unfinished static searches retain their own
/// search scratch between ticks.
#[derive(Clone, Debug)]
pub struct Walker {
    body: Body,
    /// How to get where it is going, cell by cell, orthogonally. Empty means
    /// it is not going anywhere.
    route: Box<Route>,
    /// Its own pace, cells per watched second: how fast it walks when it
    /// is in no hurry.
    speed: f32,
    /// How much of a hurry it is in, 0 (its own pace) to 1 (flat out,
    /// [`HURRY`] faster): eased towards the urgency the brain asks for
    /// ([`Walker::hurry`]), and 0 whenever it has nowhere to go.
    hurry: f32,
    /// What share of its own pace it walks at, hurry and all: 1, or less
    /// for somebody whose job says so (a king, `job::Bearing::gait`).
    /// **Not saved**: its owner sets it from the job whenever the job is
    /// handed out, taken back or loaded ([`Walker::set_gait`]).
    gait: f32,
    /// What far searching it may still do this tick, while the react step
    /// has it metered ([`Walker::meter`]); `None`, unmetered, is
    /// [`FAR_LIMIT`] a search and nothing shared — a walker driven by a test.
    /// Here and not in `route`, like `searching`: the react step asks both of
    /// every walker every tick, and the body beside them is already in cache
    /// where the boxed route may not be.
    meter: Option<Allowance>,
    /// Whether `route` holds an unfinished far search — kept in step with it
    /// by everything that starts, finishes or drops one.
    searching: bool,
}

#[cfg(test)]
thread_local! {
    /// How many far routes this thread has asked for — the counting half of
    /// `brain`'s test that a brain never asks for a route more than once a
    /// task. Per thread, so tests running side by side do not count each
    /// other's.
    pub(crate) static ROUTES_ASKED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Route buffers live together, allocated once at spawn rather than inflating
/// every unit's hot body/brain data or allocating a task queue per walk.
#[derive(Clone, Debug, Default)]
struct Route {
    path: Path,
    repair: Vec<Point>,
    pending: Option<Box<PathFinder>>,
    request: Option<RouteRequest>,
}

#[derive(Clone, Copy, Debug)]
struct RouteRequest {
    start: Point,
    goals: Destinations,
    fallback: Option<Destinations>,
    key: Option<super::property::PropertyId>,
}

impl Walker {
    pub fn new(uid: Uid, cell: Point, speed: f32) -> Walker {
        Walker {
            body: Body::at_cell(uid, cell),
            route: Box::default(),
            speed,
            hurry: 0.0,
            gait: 1.0,
            meter: None,
            searching: false,
        }
    }

    pub fn body(&self) -> &Body {
        &self.body
    }

    pub fn body_mut(&mut self) -> &mut Body {
        &mut self.body
    }

    /// Its own pace, cells per watched second, in no hurry.
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// How fast it is going right now: its own pace, and as much faster as
    /// the hurry it is in, at the gait it keeps.
    pub fn current_speed(&self) -> f32 {
        self.speed * self.gait * (1.0 + HURRY * self.hurry)
    }

    /// Walk at `gait` of its own pace from now on ([`Walker::gait`]'s
    /// field): what the job says, set by whoever holds the job.
    pub fn set_gait(&mut self, gait: f32) {
        self.gait = gait;
    }

    /// What share of its own pace it walks at.
    pub fn gait(&self) -> f32 {
        self.gait
    }

    /// How much of a hurry it is in, 0 to 1.
    pub fn hurrying(&self) -> f32 {
        self.hurry
    }

    /// **Hurry as much as `urgency` (0 to 1) says**, for one tick of `dt`
    /// watched seconds: the hurry it is in moves towards it by no more than
    /// `dt` / [`SECONDS_TO_HURRY`], up or down. With nowhere to go it is in
    /// no hurry at all, so every walk sets off at its own pace and speeds up.
    ///
    /// The brain calls it once a tick, after the task has run, with the
    /// urgency of the goal in charge; nothing else writes the hurry. A NaN
    /// asks for nothing.
    pub fn hurry(&mut self, urgency: f32, dt: f32) {
        if !self.is_walking() {
            self.hurry = 0.0;
            return;
        }
        let wanted = if urgency > 0.0 { urgency.min(1.0) } else { 0.0 };
        let most = dt / SECONDS_TO_HURRY;
        // Clamped again: `a + (b - a)` may land an ulp past `b`.
        self.hurry = (self.hurry + (wanted - self.hurry).clamp(-most, most)).clamp(0.0, 1.0);
    }

    /// **Walk the plan.** Aim at the centre of the next cell in the path, and
    /// nothing else.
    ///
    /// There is no decision left in here: the route was chosen in the reaction
    /// round, which is where the `&mut self` a route has to be stored through
    /// lives. What remains is arithmetic, which is exactly the shape the read
    /// phase wants — no RNG, no search, no branch on the crowd.
    pub fn think(&self, ctx: &Think<'_>) -> Intent {
        let Some(step) = self.route.path.current() else {
            return Intent::Idle;
        };
        // A door ahead that is not open all the way: stand and wait for it.
        // Asking for it to open is the reaction round's
        // (`TaskCtx::advance_action`), since think may not ask for anything.
        if step != self.body.center_position() && ctx.doors.is_shut(step) {
            return Intent::Idle;
        }

        let (x, y) = self.body.position();
        let (tx, ty) = (step.x as f32 + 0.5, step.y as f32 + 0.5);
        let (dx, dy) = (tx - x, ty - y);
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= ARRIVED {
            return Intent::Idle;
        }

        let step = (self.current_speed() * ctx.dt).min(distance).min(MAX_STEP);
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
        if self.route.path.len() > 1
            && self.route.path.current() == Some(self.body.center_position())
        {
            self.route.path.advance();
        }
    }

    /// Which way it is going: from where it stands to the middle of the next
    /// cell of the route. `None` with no route, or standing on its end.
    pub fn heading(&self) -> Option<(f32, f32)> {
        let step = self.route.path.current()?;
        let (x, y) = self.body.position();
        let (dx, dy) = (step.x as f32 + 0.5 - x, step.y as f32 + 0.5 - y);
        (dx * dx + dy * dy > ARRIVED * ARRIVED).then_some((dx, dy))
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
        self.route_to_any(ctx, Destinations::one(cell))
    }

    /// A multi-destination walk succeeds at the cheapest reachable approach.
    /// Budget exhaustion keeps the action running, with no movement until the
    /// static search finishes. It never becomes a false unreachable result.
    pub fn route_to_any(&mut self, ctx: &Think<'_>, goals: Destinations) -> bool {
        self.halt();
        #[cfg(test)]
        ROUTES_ASKED.with(|asked| asked.set(asked.get() + 1));
        let free = Destinations::from_cells(
            goals
                .as_slice()
                .iter()
                .copied()
                .filter(|&cell| ctx.occupancy.is_free_for(cell, self.body.uid())),
        );
        let prefer_free = goals.as_slice().len() > 1 && free.first().is_some() && free != goals;
        self.route.request = Some(RouteRequest {
            start: self.body.center_position(),
            goals: if prefer_free { free } else { goals },
            fallback: prefer_free.then_some(goals),
            key: self.body.home(),
        });
        self.continue_route(ctx) != SearchStatus::Unreachable
    }

    fn continue_route(&mut self, ctx: &Think<'_>) -> SearchStatus {
        let mut request = self.route.request.expect("a route is being planned");
        if request.start != self.body.center_position() || request.key != self.body.home() {
            path::recycle(&mut self.route.pending);
            request.start = self.body.center_position();
            request.key = self.body.home();
            self.route.request = Some(request);
        }
        let mut allowance = match self.meter {
            Some(meter) => Allowance { cells: meter.cells.min(FAR_LIMIT), ..meter },
            None => Allowance::new(FAR_LIMIT),
        };
        let given = allowance.cells;
        let status = path::search(
            &mut self.route.pending,
            ctx.map,
            request.start,
            request.goals,
            &mut allowance,
            |c| ctx.step_cost_with(c, request.key),
            self.route.path.output(),
        );
        if let Some(meter) = &mut self.meter {
            meter.cells -= given - allowance.cells;
        }
        self.searching = self.route.pending.is_some();
        match status {
            SearchStatus::Found(cell) => {
                self.route.request = None;
                if self.route.path.is_done() {
                    self.route.path.single(cell);
                }
            }
            SearchStatus::Unreachable => {
                if let Some(fallback) = request.fallback {
                    self.route.request = Some(RouteRequest {
                        goals: fallback,
                        fallback: None,
                        ..request
                    });
                    return self.continue_route(ctx);
                }
                self.route.request = None;
            }
            SearchStatus::BudgetExhausted => {}
        }
        status
    }

    /// Let this walker's far searches spend no more than `allowance` until
    /// [`Walker::unmeter`] — the react step's share of the tick's route
    /// budget, handed over before the unit reacts. What a search cannot
    /// finish inside it waits for a later tick; it is never cut short into
    /// a different route, or into a miss.
    pub fn meter(&mut self, allowance: Allowance) {
        self.meter = Some(allowance);
    }

    /// Stop metering, and say how many cells of the allowance are left.
    pub fn unmeter(&mut self) -> usize {
        self.meter.take().map_or(0, |meter| meter.cells)
    }

    /// Whether it holds an unfinished far search of its own between ticks
    /// — scratch the size of the search's window, which is why the react
    /// step bounds how many are held at once.
    pub fn is_searching(&self) -> bool {
        self.searching
    }

    /// Stop walking: drop the route, wherever it was going.
    pub fn halt(&mut self) {
        path::recycle(&mut self.route.pending);
        self.searching = false;
        self.route.request = None;
        self.route.path.clear();
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
        self.halt();
        self.route.path.single(cell);
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
        if self.route.request.is_some() {
            return match self.continue_route(ctx) {
                SearchStatus::Unreachable => ActionState::Failed,
                SearchStatus::Found(_) | SearchStatus::BudgetExhausted => ActionState::Running,
            };
        }
        if outcome.is_blocked() && !self.detour(ctx) {
            return ActionState::Failed;
        }
        // The last step, which `apply` leaves for arrival rather than entry.
        if self.route.path.len() == 1
            && self.route.path.current() == Some(self.body.center_position())
            && self.is_centred()
        {
            self.route.path.advance();
        }
        if self.route.path.is_done() {
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
    /// An intermediate rejoin cell may be occupied, since its occupant may
    /// leave before we reach it. A final destination must be free: repeatedly
    /// repairing the last step would bypass the goal's bounded wait/retry.
    pub fn detour(&mut self, ctx: &Think<'_>) -> bool {
        if self.route.path.is_done() {
            return true;
        }
        let cells = self.route.path.len().min(DETOUR_CELLS);
        let target = self.route.path.remaining().nth(cells - 1).unwrap();
        let final_target = Some(target) == self.route.path.destination();
        let uid = self.body.uid();
        let key = self.body.home();
        let here = self.body.center_position();
        // A local crowd snapshot is valid only for this tick. An exhausted
        // repair fails back to the goal's bounded wait/retry, retaining the
        // blocking body's ID. Only static far searches resume across ticks.
        self.route.repair.clear();
        let status = path::search_local(
            ctx.map,
            here,
            target,
            DETOUR_LIMIT,
            |cell| {
                ctx.step_cost_with(cell, key).filter(|_| {
                    (!final_target && cell == target) || ctx.occupancy.is_free_for(cell, uid)
                })
            },
            &mut self.route.repair,
        );
        if matches!(status, SearchStatus::Found(_)) {
            self.route.path.splice(cells, &mut self.route.repair);
            true
        } else {
            self.route.path.clear();
            false
        }
    }

    /// The door the route goes through next, when the next cell is one —
    /// open or not, since asking for an open one is what holds it open while
    /// this walker crosses. `None` standing in a doorway already: the door
    /// does not shut on somebody in it.
    pub fn door_ahead(&self, ctx: &Think<'_>) -> Option<Point> {
        let step = self.route.path.current()?;
        (step != self.body.center_position() && ctx.doors.is_door(step)).then_some(step)
    }

    /// Where the route ends, if there is one.
    pub fn destination(&self) -> Option<Point> {
        if let Some(request) = self.route.request {
            return request.goals.first();
        }
        if self.route.path.is_done() {
            None
        } else {
            self.route.path.destination()
        }
    }

    /// Whether there is any route left to walk.
    pub fn is_walking(&self) -> bool {
        self.route.request.is_some() || !self.route.path.is_done()
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
            ("path", format!("{} cells", self.route.path.len())),
            ("speed", format!("{:.2} cells/s", self.current_speed())),
            ("pace", format!("{:.2} cells/s", self.speed)),
            ("hurry", format!("{:.0}%", self.hurry * 100.0)),
        ]
        .into_iter()
        .chain((self.gait != 1.0).then(|| ("gait", format!("{:.0}% of its pace", self.gait * 100.0))))
        .collect()
    }

    /// The route still to walk, current step first — see
    /// [`super::GameEntity::planned_path`].
    pub fn path_cells(&self) -> Vec<Point> {
        self.route.path.remaining().collect()
    }

    // --- saving ---

    /// Everything about the walk that lasts from one tick to the next. Not
    /// the meter, which only exists while the react step has it lent out,
    /// and not the repair buffer, which is scratch.
    pub(crate) fn save(&self) -> pb::Walker {
        // Not the gait: the job it comes from is saved, and sets it again.
        let Walker { body, route, speed, hurry, gait: _, meter: _, searching: _ } = self;
        let Route { path, repair: _, pending, request } = &**route;
        pb::Walker {
            body: Some(body.save()),
            speed: *speed,
            path: Some(path.save()),
            request: request.map(|request| {
                let RouteRequest { start, goals, fallback, key } = request;
                pb::RouteRequest {
                    start: Some(start.into()),
                    goals: goals.save(),
                    fallback: fallback.map(|fallback| pb::Destinations { cells: fallback.save() }),
                    key: key.map(|key| key.get() as u32),
                }
            }),
            search: pending.as_ref().map(|finder| finder.save()),
            hurry: *hurry,
        }
    }

    /// The walk as saved. `revision` is the navigation revision of the map it
    /// is restored beside, which an unfinished search is stamped with so that
    /// it carries on rather than starting over.
    pub(crate) fn restore(saved: &pb::Walker, revision: u64) -> Result<Walker, Invalid> {
        let body = Body::restore(pb::required(&saved.body, "body")?)?;
        if !(saved.speed.is_finite() && saved.speed > 0.0) {
            return invalid(format!("walks at {} cells a second", saved.speed));
        }
        if !(0.0..=1.0).contains(&saved.hurry) {
            return invalid(format!("in a hurry of {}", saved.hurry));
        }
        let request = match &saved.request {
            None => None,
            Some(request) => Some(RouteRequest {
                start: pb::cell(request.start, "route start")?,
                goals: Destinations::restore(&request.goals, "route goals")?,
                fallback: request
                    .fallback
                    .as_ref()
                    .map(|fallback| Destinations::restore(&fallback.cells, "route fallback"))
                    .transpose()?,
                key: match request.key {
                    None => None,
                    Some(key) => Some(
                        u16::try_from(key)
                            .ok()
                            .and_then(super::property::PropertyId::new)
                            .ok_or_else(|| Invalid::new(format!("route key {key}")))?,
                    ),
                },
            }),
        };
        let pending = saved.search.as_ref().map(|search| PathFinder::restore(search, revision)).transpose()?;
        if pending.is_some() && request.is_none() {
            return invalid("a search for no route");
        }
        Ok(Walker {
            body,
            searching: pending.is_some(),
            route: Box::new(Route {
                path: Path::restore(pb::required(&saved.path, "path")?)?,
                repair: Vec::new(),
                pending: pending.map(Box::new),
                request,
            }),
            speed: saved.speed,
            hurry: saved.hurry,
            gait: 1.0,
            meter: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    #[cfg(test)]
    pub(crate) fn set_path(&mut self, steps: Vec<Point>) {
        self.route.path = Path::new(steps);
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.route.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Map, Size, WALL};
    use crate::sim::entity::cell_of;
    use crate::sim::facilities::Facilities;
    use crate::sim::log::Log;
    use crate::sim::occupancy::Occupancy;
    use crate::sim::uid::EntityType;

    struct World {
        map: Map,
        occupancy: Occupancy,
        log: Log,
        facilities: Facilities,
    }

    impl World {
        fn new(map: Map) -> World {
            World {
                occupancy: Occupancy::new(map.size()),
                map,
                log: Log::new(),
                facilities: Facilities::default(),
            }
        }

        fn ctx(&self, tick: u64) -> Think<'_> {
            Think::new(&self.map, &self.occupancy, &self.log, &self.facilities, 1.0 / 60.0, tick, crate::sim::clock::Clock::after_watching(0.0))
        }
    }

    fn walker(cell: Point) -> Walker {
        Walker::new(Uid::new(EntityType::Human, 42), cell, 2.0)
    }

    #[test]
    fn an_occupied_final_destination_fails_instead_of_repairing_forever() {
        let mut world = World::new(Map::new(Size::new(9, 9), FLOOR));
        let blocker = Uid::new(EntityType::Human, 99);
        let goal = Point::new(2, 1);
        world.occupancy.claim(goal, blocker).unwrap();
        let mut walker = walker(Point::new(1, 1));
        assert!(walker.route_to(&world.ctx(0), goal));
        assert_eq!(
            walker.advance(&world.ctx(0), MoveOutcome::Blocked { by: Some(blocker) }),
            ActionState::Failed
        );
        assert!(!walker.is_walking());
    }

    #[test]
    fn a_large_mixed_cost_search_waits_then_resumes_and_can_be_cancelled() {
        let road = crate::map::TerrainId::from_name("road").unwrap();
        let mut map = Map::new(Size::new(300, 300), road);
        map.set_terrain(Point::ORIGIN, FLOOR); // admissible minimum remains one
        let world = World::new(map);
        let goal = Point::new(299, 299);
        let mut walker = walker(Point::ORIGIN);
        assert!(walker.route_to(&world.ctx(0), goal));
        assert!(
            walker.route.pending.is_some(),
            "fixture must exhaust the first slice"
        );
        assert!(walker.is_searching());
        assert_eq!(walker.think(&world.ctx(0)), Intent::Idle);
        assert_eq!(
            walker.advance(&world.ctx(1), MoveOutcome::Idle),
            ActionState::Running
        );
        assert!(walker.route.pending.is_none());
        assert!(!walker.is_searching());
        assert_eq!(walker.destination(), Some(goal));
        assert_eq!(walker.route.path.len(), 598);
        assert!(matches!(walker.think(&world.ctx(2)), Intent::Move { .. }));
        assert!(walker.route_to(&world.ctx(3), goal));
        assert!(walker.route.pending.is_some());
        assert!(walker.is_searching());
        walker.halt();
        assert!(walker.route.pending.is_none());
        assert!(!walker.is_searching());
        assert!(!walker.is_walking());
    }

    /// How far one tick's step takes `walker`, in cells.
    fn stride(walker: &Walker, world: &World) -> f32 {
        let Intent::Move { to } = walker.think(&world.ctx(0)) else {
            panic!("it should be walking");
        };
        let (x, y) = walker.body().position();
        ((to.0 - x).powi(2) + (to.1 - y).powi(2)).sqrt()
    }

    #[test]
    fn a_walker_in_a_hurry_breaks_into_a_run_over_a_second_and_goes_no_faster() {
        let world = World::new(Map::new(Size::new(30, 3), FLOOR));
        let mut walker = walker(Point::new(1, 1));
        assert!(walker.route_to(&world.ctx(0), Point::new(28, 1)));
        let dt = 1.0 / 60.0;
        let calm = stride(&walker, &world);
        assert!((calm - 2.0 * dt).abs() < 1e-6, "its own pace: {calm}");

        // Half a second in, half way to a run, and no more.
        for _ in 0..30 {
            walker.hurry(1.0, dt);
        }
        assert!((walker.hurrying() - 0.5).abs() < 1e-4, "{}", walker.hurrying());
        assert!(stride(&walker, &world) > calm);

        for _ in 0..60 {
            walker.hurry(1.0, dt);
        }
        assert_eq!(walker.hurrying(), 1.0);
        let run = stride(&walker, &world);
        assert!((run - calm * (1.0 + HURRY)).abs() < 1e-6, "flat out: {run}");

        // Asked for more than all there is, or for nothing it can read.
        walker.hurry(7.0, dt);
        assert_eq!(walker.hurrying(), 1.0);
        walker.hurry(f32::NAN, dt);
        assert!(walker.hurrying() < 1.0, "a NaN is no hurry, eased down to");
    }

    #[test]
    fn a_walker_slows_back_to_its_own_pace_and_stops_hurrying_when_it_stops() {
        let world = World::new(Map::new(Size::new(30, 3), FLOOR));
        let mut walker = walker(Point::new(1, 1));
        assert!(walker.route_to(&world.ctx(0), Point::new(28, 1)));
        let dt = 1.0 / 60.0;
        for _ in 0..60 {
            walker.hurry(1.0, dt);
        }
        walker.hurry(0.0, dt);
        assert!(walker.hurrying() > 0.9, "slows down, does not stop dead: {}", walker.hurrying());
        for _ in 0..60 {
            walker.hurry(0.0, dt);
        }
        assert_eq!(walker.hurrying(), 0.0);

        for _ in 0..60 {
            walker.hurry(1.0, dt);
        }
        walker.halt();
        walker.hurry(1.0, dt);
        assert_eq!(walker.hurrying(), 0.0, "nowhere to go: the next walk sets off at its own pace");
    }

    #[test]
    fn a_hurry_is_saved_and_restored() {
        let world = World::new(Map::new(Size::new(30, 3), FLOOR));
        let mut walker = walker(Point::new(1, 1));
        assert!(walker.route_to(&world.ctx(0), Point::new(28, 1)));
        for _ in 0..20 {
            walker.hurry(1.0, 1.0 / 60.0);
        }
        let restored = Walker::restore(&walker.save(), 0).unwrap();
        assert_eq!(restored.hurrying(), walker.hurrying());
        assert_eq!(restored.current_speed(), walker.current_speed());

        let mut saved = walker.save();
        saved.hurry = 1.5;
        assert!(Walker::restore(&saved, 0).is_err(), "more than flat out is refused");
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
        assert!(
            (x - 5.5).abs() < 1e-3 && (y - 1.5).abs() < 1e-3,
            "stopped at {x}, {y}"
        );
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
        assert!(
            (x - 3.5).abs() < 1e-3 && (y - 3.5).abs() < 1e-3,
            "stopped at {x}, {y}"
        );
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
        let features =
            crate::sim::Features::from_map(&map, &crate::map::utilities::Supply::everywhere(map.size()));
        assert!(
            features.is_enterable(toilet),
            "the fixture should have made a real toilet"
        );

        let mut world = World::new(map);
        world.facilities.features = features;
        let mut walker = walker(Point::new(1, 1));

        assert!(
            !walker.route_to(&world.ctx(0), Point::new(7, 4)),
            "the toilet is not a doorway"
        );
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
        assert!(
            !walker.is_walking(),
            "a stale route must not survive a miss"
        );
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
            !walker.path().remaining().any(|p| p == Point::new(5, 4)),
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
    fn a_walker_keeps_to_the_pavement_and_crosses_the_road_straight_over() {
        // A street: a pavement each side of three cells of road. From one end
        // of the near pavement to the far end of the other, the straight line
        // runs diagonally down the road; a walker should walk the pavement and
        // cross once, at right angles.
        let pavement = crate::map::TerrainId::from_name("pavement").unwrap();
        let road = crate::map::TerrainId::from_name("road").unwrap();
        let mut map = Map::new(Size::new(16, 5), road);
        for x in 0..16 {
            map.set_terrain(Point::new(x, 0), pavement);
            map.set_terrain(Point::new(x, 4), pavement);
        }
        let world = World::new(map);
        let mut walker = walker(Point::new(0, 0));
        assert!(walker.route_to(&world.ctx(0), Point::new(15, 4)));

        let on_road: Vec<Point> = walker
            .path()
            .remaining()
            .filter(|&cell| world.map.walk_cost(cell) > 1)
            .collect();
        assert_eq!(
            on_road.len(),
            3,
            "one crossing, three cells of road: {on_road:?}"
        );
        assert!(
            on_road.iter().all(|cell| cell.x == on_road[0].x),
            "crossed at an angle: {on_road:?}"
        );
    }

    #[test]
    fn a_detour_round_a_body_on_the_pavement_steps_onto_the_road_only_to_pass() {
        let pavement = crate::map::TerrainId::from_name("pavement").unwrap();
        let road = crate::map::TerrainId::from_name("road").unwrap();
        let mut map = Map::new(Size::new(12, 4), road);
        for x in 0..12 {
            map.set_terrain(Point::new(x, 0), pavement);
        }
        let mut world = World::new(map);
        let blocker = Uid::new(EntityType::Human, 99);
        world.occupancy.claim(Point::new(3, 0), blocker).unwrap();

        let mut walker = walker(Point::new(2, 0));
        walker.set_path((3..=10).map(|x| Point::new(x, 0)).collect());
        let state = walker.advance(&world.ctx(0), MoveOutcome::Blocked { by: Some(blocker) });

        assert_eq!(state, ActionState::Running);
        let remaining: Vec<_> = walker.path().remaining().collect();
        let on_road = remaining
            .iter()
            .filter(|&&cell| world.map.walk_cost(cell) > 1)
            .count();
        assert_eq!(on_road, 3, "out, past and back: {remaining:?}");
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
