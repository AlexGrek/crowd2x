//! [`RelieveGoal`]: walk to the nearest toilet, then into it, and use it.
//!
//! Two tasks — [`MoveTo`](crate::sim::brain::tasks::MoveTo) beside the toilet,
//! [`UseToilet`](crate::sim::brain::tasks::UseToilet), which finishes the walk
//! by stepping inside — and what this goal does is choose the toilet, queue
//! them, and decide what a failure means. The bladder emptying is the task's
//! and the body's doing.
//!
//! Nothing in hand matters here: a human carrying food to the toilet carries
//! it back out again, and eats it when eating is next in charge.

use crate::map::Point;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Stand, PATIENCE, WAIT_FOR_A_GAP};

/// Five world minutes on the toilet — two and a half seconds of watching,
/// written in world units like every duration a body is watched standing
/// through (see [`crate::sim::clock`]).
pub const TOILET_SECONDS: f32 = watched(5.0 * MINUTE);

/// Use the toilet.
///
/// A toilet **is** busy while somebody is inside it: entering claims its cell
/// the same way any other movement claims a cell
/// (`crate::sim::occupancy::Occupancy`), so a second body finding it taken is
/// simply blocked, exactly as one standing in a doorway would be. What this
/// goal remembers is the toilet it chose and how many times in a row either
/// the walk there or the step inside it has been refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RelieveGoal {
    /// The toilet, by cell.
    target: Option<Point>,
    /// Walks to the toilet, or attempts to step inside it, blocked by a body
    /// so far, in a row.
    retries: u8,
}

impl RelieveGoal {
    pub fn new() -> RelieveGoal {
        RelieveGoal::default()
    }

    /// Queue the rest **from what is true now**, on the back of the queue:
    /// walk beside the toilet (unless already there), use it. `false` when
    /// there is nowhere to stand to use it.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(toilet) = self.target else {
            return false;
        };
        match stand_beside(ctx, toilet) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_to(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::use_toilet(toilet, TOILET_SECONDS));
        true
    }

    fn forget(&mut self) {
        self.target = None;
        self.retries = 0;
    }

    /// Done: say so, and start the next visit from scratch.
    fn relieved(&mut self, ctx: &GoalCtx<'_>, toilet: Point) {
        ctx.think
            .log
            .push(format!("{} used the toilet at {}, {}", ctx.body.uid(), toilet.x, toilet.y));
        self.forget();
    }
}

impl GoalExecutor for RelieveGoal {
    fn goal(&self) -> GoalId {
        GoalId::Relieve
    }

    /// The half-walked route is gone, the toilet is not.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.target.is_none() {
            return;
        }
        self.retries = 0;
        if !self.plan(ctx) {
            self.forget();
        }
    }

    /// Using the toilet empties the bladder below
    /// [`RELIEVED`](crate::sim::brain::routines::RELIEVED), so the routine
    /// lets go before this goal has heard — it hears here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() == TaskResult::Success
            && let Some(Task::UseToilet(task)) = ctx.finished
        {
            self.relieved(ctx, task.toilet);
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() {
            // A kind with no bladder.
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_)) | Some(Task::UseToilet(_))) => {
                // A body in the way of the walk there, or already inside the
                // toilet when this one tried to step in — either way,
                // something a moving body caused, so it may move again.
                ctx.tasks.clear();
                if ctx.blocked_by.is_some() && self.retries < PATIENCE && self.target.is_some() {
                    // Give them a moment, then set off again — which,
                    // starting from beside it already, is straight back to
                    // trying to step in.
                    self.retries += 1;
                    let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                    if self.plan(ctx) {
                        return GoalProgress::Working;
                    }
                }
                // No way to the toilet, no end to the crowd round it, or it
                // is still occupied after waiting: any of them means giving
                // up on this visit rather than retrying every tick.
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Standing in the wrong place. Cheap to work out again next
                // tick.
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, Some(Task::UseToilet(task))) => {
                self.relieved(ctx, task.toilet);
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
        let Some(toilet) = ctx.think.features.nearest(FeatureKind::Toilet, here) else {
            return GoalProgress::Blocked;
        };
        self.target = Some(toilet);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "toilet",
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
    use crate::sim::brain::routines::RELIEVED;
    use crate::sim::brain::GoalId;
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::{GameEntity, Human};

    /// Bursting, and nothing else pressing.
    fn bursting_human(cell: Point, bladder: f32) -> Human {
        needy_human(cell, 0.0, 0.0, bladder)
    }

    #[test]
    fn a_bursting_human_walks_to_the_toilet_and_is_relieved() {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        let toilet = Point::new(11, 7);
        prop_at(&mut map, "toilet", toilet);
        let mut world = World::new(map);
        let mut human = bursting_human(Point::new(2, 2), 90.0);

        let mut went = false;
        for _ in 0..3000 {
            world.step(&mut human);
            went |= human.brain().top_goal() == GoalId::Relieve;
            if world.reliefs() > 0 {
                break;
            }
        }
        assert!(went, "a full bladder should have put the toilet in charge");
        assert!(
            world.log_contains("used the toilet at 11, 7"),
            "never got there; the brain says {:?}",
            human.brain_fields()
        );
        assert_eq!(human.center_position(), toilet, "should be standing inside it, not beside it");
        assert!(human.stats().bladder() <= RELIEVED, "bladder {}", human.stats().bladder());

        // ...and, relieved, back to wandering.
        for _ in 0..10 {
            world.step(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
    }

    #[test]
    fn a_bursting_human_with_no_toilet_in_the_world_goes_back_to_wandering() {
        let mut world = World::new(Map::new(Size::new(12, 12), FLOOR));
        let mut human = bursting_human(Point::new(6, 6), 95.0);
        let start = human.position();

        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Relieve) > 0, "the toilet should be held off");
        assert_ne!(human.position(), start, "and it should be walking about meanwhile");
    }

    #[test]
    fn a_toilet_already_taken_is_waited_out_and_used_once_it_frees_up() {
        // Standing beside the toilet already, so what plays out is entirely
        // the contention over its own cell, not a walk across the room.
        // `occupancy.claim` by hand stands in for a second unit already
        // inside it — the same trick `walker.rs`'s detour tests use for "a
        // body in the way" without a second entity to drive.
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        let toilet = Point::new(6, 6);
        prop_at(&mut map, "toilet", toilet);
        let mut world = World::new(map);
        let occupant = Uid::new(EntityType::Human, 999);
        world.occupancy.claim(toilet, occupant).expect("empty at the start");

        let mut human = bursting_human(Point::new(5, 6), 95.0);
        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_eq!(world.reliefs(), 0, "the toilet was taken the whole time");

        world.occupancy.release(toilet, occupant);
        for _ in 0..500 {
            world.step(&mut human);
            if world.reliefs() > 0 {
                break;
            }
        }
        assert!(
            world.reliefs() > 0,
            "never got in once it was free; brain: {:?}",
            human.brain_fields()
        );
        assert_eq!(human.center_position(), toilet);
    }

    #[test]
    fn a_toilet_that_stays_taken_is_given_up_on_rather_than_retried_forever() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        let toilet = Point::new(6, 6);
        prop_at(&mut map, "toilet", toilet);
        let mut world = World::new(map);
        let occupant = Uid::new(EntityType::Human, 999);
        world.occupancy.claim(toilet, occupant).expect("empty at the start");

        // Already beside it, so no walk across the room to pay for — but the
        // first attempt at stepping in still has to cross from beside it to
        // the boundary of the toilet's cell before it can be refused, and
        // each retry after that waits out `WAIT_FOR_A_GAP` in between.
        let mut human = bursting_human(Point::new(5, 6), 95.0);
        for _ in 0..1000 {
            world.step(&mut human);
        }
        assert_eq!(world.reliefs(), 0);
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Relieve) > 0, "the toilet should be held off");
    }

    #[test]
    fn a_toilet_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for i in 7..12 {
            map.set_terrain(Point::new(i, 7), WALL);
            map.set_terrain(Point::new(7, i), WALL);
        }
        prop_at(&mut map, "toilet", Point::new(10, 10));
        let mut world = World::new(map);
        let mut human = bursting_human(Point::new(2, 2), 95.0);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..1000 {
            world.step(&mut human);
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert_eq!(world.reliefs(), 0);
        assert!(routes < 100, "{routes} routes in 1000 ticks");
    }

    #[test]
    fn a_fridge_across_a_corridor_is_a_wall_to_somebody_trying_to_reach_the_toilet_beyond_it() {
        // One cell wide: the only way to the toilet is through the fridge.
        let mut map = Map::new(Size::new(9, 1), FLOOR);
        prop_at(&mut map, "fridge", Point::new(4, 0));
        prop_at(&mut map, "toilet", Point::new(8, 0));
        let mut world = World::new(map);
        let mut human = bursting_human(Point::new(0, 0), 95.0);

        for tick in 0..2000 {
            world.step(&mut human);
            assert!(human.center_position().x < 4, "tick {tick}: got past the fridge");
        }
        assert_eq!(world.reliefs(), 0);
    }

    #[test]
    fn drinking_is_what_sends_a_human_to_the_toilet_in_a_minute_and_a_half() {
        // The same thirsty human twice, once with thirst switched off so
        // nothing makes it drink. Hunger is off in both: no meals in the way.
        let reliefs = |thirst_running: bool| {
            let mut map = Map::new(Size::new(12, 12), FLOOR);
            prop_at(&mut map, "fridge", Point::new(3, 3));
            prop_at(&mut map, "toilet", Point::new(8, 8));
            let mut world = World::new(map);
            let mut human = needy_human(Point::new(5, 5), 0.0, 90.0, 0.0);
            let biology = human.biology_mut().unwrap();
            biology.set_running(ProcessId::Hunger, false);
            biology.set_running(ProcessId::Thirst, thirst_running);
            for _ in 0..6000 {
                world.step(&mut human);
            }
            (world.drinks(), world.reliefs())
        };
        let (drinks, reliefs_after_drinking) = reliefs(true);
        assert!(drinks > 0);
        assert!(reliefs_after_drinking > 0, "{drinks} drinks and never needed the toilet");
        assert_eq!(reliefs(false), (0, 0), "time alone does not fill a bladder that fast");
    }

    #[test]
    fn a_bursting_thirsty_human_sees_to_the_worse_first_and_loses_neither() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        prop_at(&mut map, "fridge", Point::new(2, 9));
        prop_at(&mut map, "toilet", Point::new(9, 2));
        let mut world = World::new(map);
        let mut human = needy_human(Point::new(5, 5), 0.0, 75.0, 95.0);
        human.biology_mut().unwrap().set_running(ProcessId::Hunger, false);

        let mut first = None;
        for _ in 0..4000 {
            world.step(&mut human);
            if first.is_none() && matches!(human.brain().top_goal(), GoalId::Drink | GoalId::Relieve) {
                first = Some(human.brain().top_goal());
            }
            if world.reliefs() > 0 && world.drinks() > 0 {
                break;
            }
        }
        assert_eq!(first, Some(GoalId::Relieve), "the bladder was the worse of the two");
        assert!(world.reliefs() > 0 && world.drinks() > 0, "brain: {:?}", human.brain_fields());
        let line = |needle| world.lines().iter().position(|line| line.contains(needle));
        assert!(line("used the toilet") < line("drank at the fridge"));
    }
}
