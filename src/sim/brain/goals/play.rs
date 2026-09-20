//! [`PlayGoal`]: walk to the nearest computer and have a go on it.
//!
//! Two tasks — [`MoveTo`](crate::sim::brain::tasks::MoveTo) beside the
//! computer, [`UseComputer`](crate::sim::brain::tasks::UseComputer) — and what
//! this goal does is choose the computer, queue them, and decide what a
//! failure means. The fun going back up is the task's and the body's doing.
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

use crate::map::Point;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Stand, PATIENCE, WAIT_FOR_A_GAP};

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

/// Have a go on a computer.
///
/// What it remembers is the computer it chose and how many times in a row the
/// walk there has been refused by a body in the way.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PlayGoal {
    /// The computer, by cell.
    target: Option<Point>,
    /// Walks to it blocked by a body so far, in a row.
    retries: u8,
}

impl PlayGoal {
    pub fn new() -> PlayGoal {
        PlayGoal::default()
    }

    /// Queue the rest **from what is true now**, on the back of the queue:
    /// walk beside the computer (unless already there), use it. `false` when
    /// there is nowhere to stand to use it.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(computer) = self.target else {
            return false;
        };
        match stand_beside(ctx, computer) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_to(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::use_computer(computer, PLAY_SECONDS));
        true
    }

    fn forget(&mut self) {
        self.target = None;
        self.retries = 0;
    }

    /// The go is over: say so, and start the next one from scratch.
    fn played(&mut self, ctx: &GoalCtx<'_>, computer: Point) {
        ctx.think.log.push(format!(
            "{} had a go on the computer at {}, {}",
            ctx.body.uid(),
            computer.x,
            computer.y
        ));
        self.forget();
    }
}

impl GoalExecutor for PlayGoal {
    fn goal(&self) -> GoalId {
        GoalId::Play
    }

    /// The half-walked route is gone, the computer is not.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.target.is_none() {
            return;
        }
        self.retries = 0;
        if !self.plan(ctx) {
            self.forget();
        }
    }

    /// A go that ends takes boredom under
    /// [`AMUSED`](crate::sim::brain::routines::AMUSED), so the routine lets go
    /// before this goal has heard — it hears here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() == TaskResult::Success
            && let Some(Task::UseComputer(task)) = ctx.finished
        {
            self.played(ctx, task.computer);
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
                if ctx.blocked_by.is_some() && self.retries < PATIENCE && self.target.is_some() {
                    // Somebody in the way of the desk: bodies move. Give them
                    // a moment, then set off again.
                    self.retries += 1;
                    let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                    if self.plan(ctx) {
                        return GoalProgress::Working;
                    }
                }
                // No way there, or no end to the crowd round it: give up on
                // this go rather than retrying every tick.
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
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.retries = 0,
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again.
        let here = ctx.body.center_position();
        let Some(computer) = ctx.think.features.nearest(FeatureKind::Entertainment, here) else {
            return GoalProgress::Blocked;
        };
        self.target = Some(computer);
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
            match self.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "none".to_string(),
            },
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR, WALL};
    use crate::sim::biology::ProcessId;
    use crate::sim::brain::routines::AMUSED;
    use crate::sim::brain::GoalId;
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::{GameEntity, Human};

    /// Bored, and nothing else pressing.
    fn bored_human(cell: Point, boredom: f32) -> Human {
        let mut human = needy_human(cell, 0.0, 0.0, 0.0);
        human.set_fun(100.0 - boredom);
        human
    }

    #[test]
    fn a_bored_human_walks_to_the_computer_and_has_a_go_on_it() {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
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
        let mut map = Map::new(Size::new(14, 10), FLOOR);
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
    fn a_computer_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for i in 7..12 {
            map.set_terrain(Point::new(i, 7), WALL);
            map.set_terrain(Point::new(7, i), WALL);
        }
        prop_at(&mut map, "computer", Point::new(10, 10));
        let mut world = World::new(map);
        let mut human = bored_human(Point::new(2, 2), 95.0);

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
        let mut map = Map::new(Size::new(12, 12), FLOOR);
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
        let mut map = Map::new(Size::new(12, 12), FLOOR);
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
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        prop_at(&mut map, "fridge", Point::new(2, 9));
        prop_at(&mut map, "computer", Point::new(9, 2));
        let mut world = World::new(map);
        // Starving, and as bored as it is possible to be: hunger at 100 and
        // boredom at 95 are 2.0 against 1.9 on the one priority scale.
        let mut human = needy_human(Point::new(5, 5), 100.0, 0.0, 0.0);
        human.set_fun(5.0);

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
}
