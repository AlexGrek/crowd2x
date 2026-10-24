//! [`PlayGoal`]: walk to the nearest computer or TV and have a go on it.
//!
//! Two tasks — [`MoveTo`](crate::sim::brain::tasks::MoveTo) beside the
//! computer, [`UseComputer`](crate::sim::brain::tasks::UseComputer); or
//! `MoveTo` a seat in view of the TV,
//! [`WatchTv`](crate::sim::brain::tasks::WatchTv) — and what this goal does
//! is choose which, queue them, and decide what a failure means. The fun
//! going back up is the task's and the body's doing. Whichever of the two is
//! nearer wins: both answer the one need, boredom, so they are one goal.
//!
//! Nothing in hand matters here: a human that carried its lunch to the desk
//! carries it away again, and eats it when eating is next in charge.
//!
//! A computer is used from **beside** it, like a fridge and unlike the
//! toilet, so several people can take turns at one desk without any of them
//! ever standing in its cell — and, for the same reason, a computer somebody
//! else is already using is not *busy* as far as this goal is concerned.
//! Whoever gets there sits down; the screen says which of them that is
//! (`game::props`).
//!
//! A TV is watched from **across the room**
//! ([`Access::Watched`](crate::sim::feature::Access::Watched)): from any cell
//! within [`VIEWING_DISTANCE`](crate::sim::feature::VIEWING_DISTANCE) of it with a clear line of sight
//! ([`can_watch`]). Where to sit is chosen here, from those 28 cells
//! ([`seat_in_view`]) — a geometric check of a fixed handful of cells and an
//! O(1) reachability lookup each, never a route search — and any number of
//! people may watch one set together.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::{can_watch, FeatureKind, VIEWING_OFFSETS};

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::ARCADE_SECONDS;
use super::{choose_nearest, choose_nearest_where, nearest_in_reach, nearest_in_reach_where, stand_beside, Seek, Stand};

/// Half an hour at the computer — fifteen seconds of watching it, written in
/// world units like every duration a body is watched standing through (see
/// [`crate::sim::clock`]).
///
/// The longest thing anybody does so far, and deliberately: a meal is a
/// quarter of an hour and the toilet is five minutes, but having a go at
/// something is what a person does when nothing else is pressing, and it
/// should read on screen as somebody settling in rather than as another
/// errand.
pub const PLAY_SECONDS: f32 = watched(30.0 * MINUTE);

/// The nearest of the places offered, as the crow flies along the grid; ties
/// to the lower cell, so the answer does not depend on which was asked first.
fn nearer(ctx: &GoalCtx<'_>, places: [Option<Point>; 3]) -> Option<Point> {
    let here = ctx.body.center_position();
    places.into_iter().flatten().min_by_key(|&cell| (here.manhattan_distance(cell), cell))
}

/// Whether the arcade cabinet in `cabinet` works right now: its playzone has
/// an administrator at its computer ([`crate::sim::Playzones::runs`]). A
/// cabinet that does not is no place to go.
fn works(ctx: &GoalCtx<'_>) -> impl Fn(Point) -> bool {
    let playzones = ctx.think.playzones;
    move |cabinet| playzones.runs(cabinet)
}

/// The nearest computer, TV or working arcade cabinet this unit does not
/// remember as out of reach.
fn amusement_in_reach(ctx: &GoalCtx<'_>) -> Option<Point> {
    nearer(
        ctx,
        [
            nearest_in_reach(ctx, FeatureKind::Entertainment),
            nearest_in_reach(ctx, FeatureKind::Television),
            nearest_in_reach_where(ctx, FeatureKind::Arcade, works(ctx)),
        ],
    )
}

/// Where a visit starts: [`amusement_in_reach`], or — when every one was
/// given up on lately — [`choose_nearest`]'s pick of any kind.
fn choose_amusement(ctx: &GoalCtx<'_>) -> Option<Point> {
    amusement_in_reach(ctx).or_else(|| {
        nearer(
            ctx,
            [
                choose_nearest(ctx, FeatureKind::Entertainment),
                choose_nearest(ctx, FeatureKind::Television),
                choose_nearest_where(ctx, FeatureKind::Arcade, works(ctx)),
            ],
        )
    })
}

/// Where to sit to watch the TV in `tv`: a cell this unit may stand in and
/// may reach (the map's O(1) component lookup), from which [`can_watch`] says
/// the picture is in sight. A free seat before one somebody is standing in,
/// then the nearest to where this unit is, then the lower cell. `None` when
/// there is no such cell.
///
/// 28 cells at most, each a few terrain reads and an occupancy lookup: the
/// handful of named cells a goal may read, never a search.
pub(super) fn seat_in_view(ctx: &GoalCtx<'_>, tv: Point) -> Option<Point> {
    let here = ctx.body.center_position();
    let key = ctx.body.home();
    let me = ctx.body.uid();
    let navigation = ctx.think.map.navigation();
    VIEWING_OFFSETS
        .iter()
        .map(|&(dx, dy)| tv.offset(dx.into(), dy.into()))
        .filter(|&seat| {
            ctx.think.is_passable_with(seat, key)
                && ctx.think.may_use(seat, key)
                && navigation.may_reach(here, seat)
                && can_watch(ctx.think.map, seat, tv)
        })
        .min_by_key(|&seat| {
            let taken = ctx.think.occupancy.occupant(seat).is_some_and(|uid| uid != me);
            (taken, here.manhattan_distance(seat), seat)
        })
}

/// Have a go on a computer, or watch a TV.
///
/// What it remembers is the computer or TV it chose and how many times in a
/// row the walk there has been refused by a body in the way. Which of the two
/// it chose is the feature index's to say, so it is not stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PlayGoal {
    /// Which place it is going to, and how the way there has gone.
    seek: Seek,
}

impl PlayGoal {
    pub fn new() -> PlayGoal {
        PlayGoal::default()
    }

    /// Queue the rest **from what is true now**, on the back of the queue:
    /// walk beside the computer (unless already there), use it; or walk to a
    /// seat in view of the TV (unless it is already in view), watch it.
    /// `false` when there is nowhere to stand to use it.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(computer) = self.seek.target else {
            return false;
        };
        if ctx.think.features.has(FeatureKind::Arcade, computer) {
            let cabinet = computer;
            // Dark: the administrator is gone, so is the game.
            if !ctx.think.playzones.runs(cabinet) {
                return false;
            }
            match stand_beside(ctx, cabinet) {
                Stand::Nowhere => return false,
                Stand::Here => {}
                Stand::At(cell) => {
                    let _ = ctx.tasks.push_back(Task::move_beside(cell));
                }
            }
            let _ = ctx.tasks.push_back(Task::use_arcade(cabinet, ARCADE_SECONDS));
            return true;
        }
        if ctx.think.features.has(FeatureKind::Television, computer) {
            let tv = computer;
            if !can_watch(ctx.think.map, ctx.body.center_position(), tv) {
                let Some(seat) = seat_in_view(ctx, tv) else {
                    return false;
                };
                let _ = ctx.tasks.push_back(Task::move_to(seat));
            }
            let _ = ctx.tasks.push_back(Task::watch_tv(tv, PLAY_SECONDS));
            return true;
        }
        match stand_beside(ctx, computer) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::use_computer(computer, PLAY_SECONDS));
        true
    }

    fn forget(&mut self) {
        self.seek.forget();
    }

    /// The go is over: say so, and start the next one from scratch.
    fn played(&mut self, ctx: &GoalCtx<'_>, computer: Point) {
        ctx.think.log.unit(ctx.body.uid(), format!(
            "{} had a go on the computer at {}, {}",
            ctx.body.uid(),
            computer.x,
            computer.y
        ));
        self.forget();
    }

    /// The game is over: say so, and start the next one from scratch.
    fn played_arcade(&mut self, ctx: &GoalCtx<'_>, cabinet: Point) {
        ctx.think.log.unit(ctx.body.uid(), format!(
            "{} played the arcade cabinet at {}, {}",
            ctx.body.uid(),
            cabinet.x,
            cabinet.y
        ));
        self.forget();
    }

    /// The programme is over: say so, and start the next one from scratch.
    fn watched(&mut self, ctx: &GoalCtx<'_>, tv: Point) {
        ctx.think.log.unit(ctx.body.uid(), format!("{} watched the TV at {}, {}", ctx.body.uid(), tv.x, tv.y));
        self.forget();
    }
}

impl GoalExecutor for PlayGoal {
    fn goal(&self) -> GoalId {
        GoalId::Play
    }

    fn save(&self) -> pb::Executor {
        let PlayGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// The half-walked route is gone, the computer is not.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.seek.target.is_none() {
            return;
        }
        self.seek.steady();
        if !self.plan(ctx) {
            self.forget();
        }
    }

    /// A go that ends takes boredom under
    /// [`AMUSED`](crate::sim::brain::routines::AMUSED), so the routine lets go
    /// before this goal has heard — it hears here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() != TaskResult::Success {
            return;
        }
        match ctx.finished {
            Some(Task::UseComputer(task)) => self.played(ctx, task.computer),
            Some(Task::UseArcade(task)) => self.played_arcade(ctx, task.cabinet),
            Some(Task::WatchTv(task)) => self.watched(ctx, task.tv),
            _ => {}
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() {
            // A kind that cannot be bored.
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there, or
                // no end to the crowd: remember it as out of reach and try
                // the next one (see `Seek`).
                if self.seek.setback(ctx, amusement_in_reach) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                // Nowhere left to try: give up on this visit rather than
                // retrying every tick.
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Pulled away from the desk part way through. Cheap to work
                // out again next tick.
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, Some(Task::UseComputer(task))) => {
                self.played(ctx, task.computer);
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::UseArcade(task))) => {
                self.played_arcade(ctx, task.cabinet);
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::WatchTv(task))) => {
                self.watched(ctx, task.tv);
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again.
        let Some(computer) = choose_amusement(ctx) else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(computer);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "computer",
            match self.seek.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "none".to_string(),
            },
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::biology::ProcessId;
    use crate::sim::brain::routines::AMUSED;
    use crate::sim::brain::GoalId;
    use crate::sim::testing::{home_map, needy_human, prop_at, World};
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::{GameEntity, Human};

    /// Bored, and nothing else pressing.
    fn bored_human(cell: Point, boredom: f32) -> Human {
        let mut human = needy_human(cell, 0.0, 0.0, 0.0);
        human.set_fun(100.0 - boredom);
        human.body_mut().set_home(crate::sim::PropertyId::new(1));
        human
    }

    #[test]
    fn a_bored_human_walks_to_the_computer_and_has_a_go_on_it() {
        let mut map = home_map(Size::new(14, 10));
        let computer = Point::new(11, 7);
        prop_at(&mut map, "computer", computer);
        let mut world = World::new(map);
        // Bored enough that one go sees to it: a single session is worth
        // `AMUSEMENT`, the way one meal is worth what is in it.
        let mut human = bored_human(Point::new(2, 2), 70.0);

        let mut went = false;
        for _ in 0..6000 {
            world.step(&mut human);
            went |= human.brain().top_goal() == GoalId::Play;
            if world.plays() > 0 {
                break;
            }
        }
        assert!(went, "boredom should have put the computer in charge");
        assert!(
            world.log_contains("had a go on the computer at 11, 7"),
            "never got there; the brain says {:?}",
            human.brain_fields()
        );
        assert_eq!(
            human.center_position().manhattan_distance(computer),
            1,
            "should be sitting beside it, not standing on it"
        );
        assert!(human.stats().boredom() <= AMUSED, "boredom {}", human.stats().boredom());

        // ...and, entertained, back to wandering.
        for _ in 0..10 {
            world.step(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
    }

    /// One go is worth [`AMUSEMENT`](crate::sim::biology::fun::AMUSEMENT) and
    /// no more, so somebody who was thoroughly bored goes back for a second
    /// one rather than being left a point short of looking for something to
    /// do — the same way a starving human eats twice.
    #[test]
    fn a_thoroughly_bored_human_has_a_second_go_rather_than_stopping_just_short() {
        let mut map = home_map(Size::new(14, 10));
        prop_at(&mut map, "computer", Point::new(11, 7));
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(9, 7), 100.0);

        for _ in 0..8000 {
            world.step(&mut human);
            if world.plays() >= 2 {
                break;
            }
        }
        assert!(world.plays() >= 2, "only {} go(es); {:?}", world.plays(), human.brain_fields());
        assert!(human.stats().boredom() <= AMUSED, "boredom {}", human.stats().boredom());
    }

    #[test]
    fn a_bored_human_with_no_computer_in_the_world_goes_back_to_wandering() {
        let mut world = World::new(Map::new(Size::new(12, 12), FLOOR));
        let mut human = bored_human(Point::new(6, 6), 95.0);
        let start = human.position();

        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Play) > 0, "the computer should be held off");
        assert_ne!(human.position(), start, "and it should be wandering meanwhile");
    }

    #[test]
    fn a_bored_human_never_games_on_an_office_computer() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        prop_at(&mut map, "computer", Point::new(6, 6));
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(5, 6), 100.0);
        human.body_mut().set_home(None);

        for _ in 0..2000 {
            world.step(&mut human);
            assert!(!matches!(human.brain().current_task(), Some(Task::UseComputer(_))));
        }
        assert_eq!(world.plays(), 0);
        assert!(human.stats().fun() < 1.0);
    }

    #[test]
    fn a_computer_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        let mut map = home_map(Size::new(12, 12));
        for i in 7..11 {
            prop_at(&mut map, "crate", Point::new(i, 7));
            prop_at(&mut map, "crate", Point::new(7, i));
        }
        prop_at(&mut map, "computer", Point::new(9, 9));
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(2, 2), 95.0);

        assert!(world.features.has(FeatureKind::Entertainment, Point::new(9, 9)), "the target exists but is unreachable");

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..1000 {
            world.step(&mut human);
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert_eq!(world.plays(), 0);
        assert!(routes < 100, "{routes} routes in 1000 ticks");
    }

    /// One desk, somebody already at it: a computer is used from beside it,
    /// so the next person sits down on another side rather than waiting for
    /// the first to leave — which is exactly what a toilet does instead.
    ///
    /// `occupancy.claim` by hand stands in for that first person, the same
    /// trick `relieve.rs` and `walker.rs` use for a body in the way without a
    /// second entity to drive.
    #[test]
    fn somebody_already_at_the_desk_is_sat_down_beside_rather_than_queued_behind() {
        let mut map = home_map(Size::new(12, 12));
        let computer = Point::new(6, 6);
        prop_at(&mut map, "computer", computer);
        let mut world = World::new(map);
        let taken = Point::new(5, 6);
        let occupant = Uid::new(EntityType::Human, 999);
        world.occupancy.claim(taken, occupant).expect("empty at the start");

        let mut human = bored_human(Point::new(2, 6), 95.0);
        for _ in 0..6000 {
            world.step(&mut human);
            if world.plays() > 0 {
                break;
            }
        }
        assert!(world.plays() > 0, "never got a go; brain: {:?}", human.brain_fields());
        let sat_at = human.center_position();
        assert_eq!(sat_at.manhattan_distance(computer), 1, "beside the computer");
        assert_ne!(sat_at, taken, "and not on top of whoever was already there");
    }

    #[test]
    fn a_human_whose_fun_is_switched_off_never_looks_for_a_computer() {
        let mut map = home_map(Size::new(12, 12));
        prop_at(&mut map, "computer", Point::new(6, 6));
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(2, 2), 100.0);
        human.biology_mut().unwrap().set_running(ProcessId::Fun, false);

        for _ in 0..2000 {
            world.step(&mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Play);
        }
        assert_eq!(world.plays(), 0);
    }

    #[test]
    fn hunger_comes_before_a_go_on_the_computer_and_neither_is_lost() {
        let mut map = home_map(Size::new(12, 12));
        prop_at(&mut map, "fridge", Point::new(2, 9));
        prop_at(&mut map, "computer", Point::new(9, 2));
        let mut world = World::new(map);
        // Starving, and as bored as it is possible to be: hunger at 100 and
        // boredom at 95 are 2.0 against 1.9 on the one priority scale.
        let mut human = needy_human(Point::new(5, 5), 100.0, 0.0, 0.0);
        human.set_fun(5.0);
        human.body_mut().set_home(crate::sim::PropertyId::new(1));

        let mut first = None;
        for _ in 0..8000 {
            world.step(&mut human);
            if first.is_none() && matches!(human.brain().top_goal(), GoalId::Eat | GoalId::Play) {
                first = Some(human.brain().top_goal());
            }
            if world.meals() > 0 && world.plays() > 0 {
                break;
            }
        }
        assert_eq!(first, Some(GoalId::Eat), "an empty stomach was the worse of the two");
        assert!(world.meals() > 0 && world.plays() > 0, "brain: {:?}", human.brain_fields());
    }

    /// Step until the log says a programme was watched, or `ticks` run out.
    fn watch_until_done(world: &mut World, human: &mut Human, ticks: usize) -> bool {
        for _ in 0..ticks {
            world.step(human);
            if world.log_contains("watched the TV at") {
                return true;
            }
        }
        false
    }

    /// A TV is watched from across the room: the nearest seat in view of it
    /// is up to three cells off, and nobody walks up to the set itself.
    #[test]
    fn a_bored_human_watches_the_tv_from_a_seat_in_view_rather_than_beside_it() {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        let tv = Point::new(11, 7);
        prop_at(&mut map, "tv", tv);
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(2, 2), 70.0);

        assert!(watch_until_done(&mut world, &mut human, 6000), "{:?}", human.brain_fields());
        assert!(world.log_contains("watched the TV at 11, 7"));
        let seat = human.center_position();
        assert!(can_watch(&world.map, seat, tv), "watched from {seat:?}");
        assert!(seat.manhattan_distance(tv) > 1, "sat down at the first seat in view, not beside the set: {seat:?}");
        assert!(human.stats().boredom() <= AMUSED, "boredom {}", human.stats().boredom());
        assert_eq!(world.plays(), 0, "not a go on a computer");
    }

    /// Three cells off, but with a wall in between, is not in view: it goes
    /// round to the side the picture can be seen from.
    #[test]
    fn a_tv_behind_a_wall_is_watched_from_its_own_side_of_the_wall() {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        for y in 0..9 {
            map.set_terrain(Point::new(8, y), crate::map::WALL);
        }
        let tv = Point::new(10, 4);
        prop_at(&mut map, "tv", tv);
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(7, 4), 70.0);
        assert!(!can_watch(&world.map, Point::new(7, 4), tv), "three away, through a wall");

        assert!(watch_until_done(&mut world, &mut human, 6000), "{:?}", human.brain_fields());
        let seat = human.center_position();
        assert!(seat.x > 8, "watched from the TV's side of the wall, not through it: {seat:?}");
        assert!(can_watch(&world.map, seat, tv));
    }

    /// A set walled in on every side has no seat in view: the goal is given
    /// up and held off, rather than asked for again every tick.
    #[test]
    fn a_tv_with_nowhere_in_sight_of_it_to_sit_is_given_up_on() {
        let mut map = Map::new(Size::new(14, 14), FLOOR);
        let tv = Point::new(9, 9);
        for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            map.set_terrain(tv.offset(dx, dy), crate::map::WALL);
        }
        prop_at(&mut map, "tv", tv);
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(2, 2), 95.0);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..1000 {
            world.step(&mut human);
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert!(!world.log_contains("watched the TV"));
        assert!(human.brain().goals().cooldown(GoalId::Play) > 0 || human.brain().top_goal() != GoalId::Play);
        assert!(routes < 100, "{routes} routes in 1000 ticks");
    }

    /// A computer and a TV are the one need: whichever is nearer is had.
    #[test]
    fn the_nearer_of_a_computer_and_a_tv_is_the_one_that_is_had() {
        let mut map = home_map(Size::new(14, 12));
        prop_at(&mut map, "computer", Point::new(11, 9));
        prop_at(&mut map, "tv", Point::new(3, 3));
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(3, 7), 70.0);

        assert!(watch_until_done(&mut world, &mut human, 6000), "{:?}", human.brain_fields());
        assert_eq!(world.plays(), 0, "the computer was further away");
    }

    // --- the arcade ---

    const COMPUTER: Point = Point::new(2, 10);
    const CABINET: Point = Point::new(11, 7);

    /// A room with one working arcade cabinet and the playzone computer that
    /// runs it, and `extra` on top.
    fn arcade(extra: &[(&str, Point)]) -> World {
        let mut map = Map::new(Size::new(14, 12), FLOOR);
        prop_at(&mut map, "arcade cabinet", CABINET);
        prop_at(&mut map, "playzone computer", COMPUTER);
        for &(name, cell) in extra {
            prop_at(&mut map, name, cell);
        }
        World::new(map)
    }

    /// One tick with the playzone run, as its administrator would say so
    /// every tick (`Effect::Operate`).
    fn step_run(world: &mut World, human: &mut Human) {
        world.playzones.run(COMPUTER);
        world.step(human);
    }

    #[test]
    fn a_bored_human_walks_to_a_working_arcade_cabinet_and_plays_a_game_on_it() {
        let mut world = arcade(&[]);
        let mut human = bored_human(Point::new(2, 2), 70.0);

        let mut played = false;
        for _ in 0..6000 {
            step_run(&mut world, &mut human);
            played |= human.interacting_with() == Some(CABINET);
            if world.arcade_games() > 0 {
                break;
            }
        }
        assert!(played, "never started a game; the brain says {:?}", human.brain_fields());
        assert!(world.log_contains("played the arcade cabinet at 11, 7"), "{:?}", human.brain_fields());
        assert_eq!(human.center_position().manhattan_distance(CABINET), 1, "beside it, not on it");
        assert!(human.stats().boredom() <= AMUSED, "boredom {}", human.stats().boredom());
        assert_eq!(world.plays(), 0, "not a go on a computer");
    }

    /// No administrator at the computer, no game: a cabinet nobody runs is
    /// not somewhere to go, however bored.
    #[test]
    fn a_dark_arcade_is_no_place_to_go() {
        let mut world = arcade(&[]);
        let mut human = bored_human(Point::new(9, 7), 95.0);
        let start = human.position();

        for _ in 0..1500 {
            world.step(&mut human);
            assert_ne!(human.interacting_with(), Some(CABINET), "playing a dead cabinet");
        }
        assert_eq!(world.arcade_games(), 0);
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Play) > 0, "held off, not retried every tick");
        assert_ne!(human.position(), start, "wandering meanwhile");
    }

    /// The administrator steps away in the middle of a game: every game in
    /// the playzone is over, with none of the fun; and when they are back
    /// the human plays again.
    #[test]
    fn the_administrator_leaving_ends_a_game_in_progress_and_it_is_played_again_when_they_are_back() {
        let mut world = arcade(&[]);
        let mut human = bored_human(Point::new(2, 2), 70.0);

        let mut playing = false;
        for _ in 0..6000 {
            step_run(&mut world, &mut human);
            if human.interacting_with() == Some(CABINET) {
                playing = true;
                break;
            }
        }
        assert!(playing, "never started; {:?}", human.brain_fields());
        let fun = human.stats().fun();
        for _ in 0..3 {
            step_run(&mut world, &mut human);
        }

        for _ in 0..300 {
            world.step(&mut human);
            assert_ne!(human.interacting_with(), Some(CABINET), "still playing with nobody at the computer");
        }
        assert_eq!(world.arcade_games(), 0, "a game cut short is not played");
        assert!(human.stats().fun() <= fun, "and no fun from it: {} after {fun}", human.stats().fun());

        for _ in 0..8000 {
            step_run(&mut world, &mut human);
            if world.arcade_games() > 0 {
                break;
            }
        }
        assert!(world.arcade_games() > 0, "never played once it was run again; {:?}", human.brain_fields());
        assert!(human.stats().boredom() <= AMUSED, "boredom {}", human.stats().boredom());
    }

    /// The arcade is one more place to be amused, and whichever is nearest is
    /// had: a TV beside the human beats a cabinet across the room, and a
    /// cabinet beside it beats a TV across the room — but a cabinet that is
    /// dark is not in the running at all.
    #[test]
    fn the_nearest_of_a_working_cabinet_and_a_tv_is_the_one_had_and_a_dark_cabinet_is_not_in_the_running() {
        let tv = Point::new(3, 3);
        let mut world = arcade(&[("tv", tv)]);
        let mut human = bored_human(Point::new(3, 6), 70.0);
        for _ in 0..6000 {
            step_run(&mut world, &mut human);
            if world.log_contains("watched the TV") || world.arcade_games() > 0 {
                break;
            }
        }
        assert!(world.log_contains("watched the TV at 3, 3") && world.arcade_games() == 0, "{:?}", human.brain_fields());

        let mut world = arcade(&[("tv", tv)]);
        let mut human = bored_human(Point::new(10, 9), 70.0);
        for _ in 0..6000 {
            step_run(&mut world, &mut human);
            if world.log_contains("watched the TV") || world.arcade_games() > 0 {
                break;
            }
        }
        assert!(world.arcade_games() == 1 && !world.log_contains("watched the TV"), "{:?}", human.brain_fields());

        // Beside the cabinet, but nobody runs it: the TV across the room it is.
        let mut world = arcade(&[("tv", tv)]);
        let mut human = bored_human(Point::new(10, 9), 70.0);
        for _ in 0..6000 {
            world.step(&mut human);
            if world.log_contains("watched the TV") {
                break;
            }
        }
        assert!(world.log_contains("watched the TV at 3, 3") && world.arcade_games() == 0, "{:?}", human.brain_fields());
    }
}
