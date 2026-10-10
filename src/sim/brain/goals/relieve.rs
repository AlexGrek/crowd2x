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
use super::{choose_nearest, nearest_in_reach, stand_beside, Seek, Stand};

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
///
/// A toilet it gives up on is remembered as out of reach for a while
/// ([`crate::sim::brain::memory::OutOfReach`]) and the next nearest is tried
/// instead, so a crowd at one door spreads out over the others.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RelieveGoal {
    /// Which place it is going to, and how the way there has gone.
    seek: Seek,
}

impl RelieveGoal {
    pub fn new() -> RelieveGoal {
        RelieveGoal::default()
    }

    /// Queue the rest **from what is true now**, on the back of the queue:
    /// walk beside the toilet (unless already there), use it. `false` when
    /// there is nowhere to stand to use it.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(toilet) = self.seek.target else {
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
        self.seek.forget();
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
        if self.seek.target.is_none() {
            return;
        }
        self.seek.steady();
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
                // A body in the way: wait and try again. No way there, or
                // no end to the crowd: remember it as out of reach and try
                // the next one (see `Seek`).
                if self.seek.setback(ctx, |ctx| nearest_in_reach(ctx, FeatureKind::Toilet)) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                // Nowhere left to try: give up on this visit rather than
                // retrying every tick.
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
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again.
        let Some(toilet) = choose_nearest(ctx, FeatureKind::Toilet) else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(toilet);
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
    use crate::map::{Map, Size, FLOOR, WALL};
    use crate::sim::biology::ProcessId;
    use crate::sim::brain::routines::RELIEVED;
    use crate::sim::brain::memory::OUT_OF_REACH_FOR;
    use crate::sim::brain::GoalId;
    use crate::sim::clock::{Clock, TIME_SCALE};
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
    fn a_toilet_that_stays_taken_is_remembered_and_the_next_nearest_is_used_instead() {
        let mut map = Map::new(Size::new(16, 12), FLOOR);
        let near = Point::new(6, 6);
        let far = Point::new(13, 6);
        prop_at(&mut map, "toilet", near);
        prop_at(&mut map, "toilet", far);
        let mut world = World::new(map);
        let occupant = Uid::new(EntityType::Human, 999);
        world.occupancy.claim(near, occupant).expect("empty at the start");

        let mut human = bursting_human(Point::new(5, 6), 95.0);
        let mut held_off = false;
        for _ in 0..2000 {
            world.step(&mut human);
            held_off |= human.brain().goals().cooldown(GoalId::Relieve) > 0;
            if world.reliefs() > 0 {
                break;
            }
        }
        assert!(world.log_contains("gave up on reaching 6, 6"), "lines: {:?}", world.lines());
        assert!(!held_off, "should have gone straight on to the next toilet, not given up the visit");
        assert!(
            world.log_contains("used the toilet at 13, 6"),
            "should have gone on to the other toilet; brain: {:?}",
            human.brain_fields()
        );
        let now = world.clock.elapsed();
        assert!(human.brain().memory().out_of_reach().contains(near, now));
    }

    #[test]
    fn a_walled_off_toilet_is_passed_over_for_one_that_can_be_reached() {
        // The nearer toilet, as the crow flies, is sealed in a room.
        let mut map = Map::new(Size::new(16, 12), FLOOR);
        for i in 0..5 {
            map.set_terrain(Point::new(i, 4), WALL);
            map.set_terrain(Point::new(4, i), WALL);
        }
        prop_at(&mut map, "toilet", Point::new(1, 1));
        prop_at(&mut map, "toilet", Point::new(14, 10));
        let mut world = World::new(map);
        let mut human = bursting_human(Point::new(5, 5), 95.0);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        let mut held_off = false;
        for _ in 0..3000 {
            world.step(&mut human);
            held_off |= human.brain().goals().cooldown(GoalId::Relieve) > 0;
            if world.reliefs() > 0 {
                break;
            }
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert!(world.log_contains("used the toilet at 14, 10"), "brain: {:?}", human.brain_fields());
        assert!(!held_off, "the sealed toilet should have been swapped for the other on the spot");
        assert!(routes < 20, "{routes} routes for one rerouted visit");
    }

    #[test]
    fn a_toilet_given_up_on_is_avoided_until_the_memory_of_it_fades() {
        // Where the same bursting human ends up, having given up on the near
        // toilet an hour of world ago less `early` seconds.
        let goes_to = |early: f64| {
            let mut map = Map::new(Size::new(16, 12), FLOOR);
            prop_at(&mut map, "toilet", Point::new(6, 6));
            prop_at(&mut map, "toilet", Point::new(13, 6));
            let mut world = World::new(map);
            let mut human = bursting_human(Point::new(5, 6), 95.0);
            human.brain_mut().memory_mut().out_of_reach_mut().remember(Point::new(6, 6), 0.0);
            world.clock = Clock::after_watching((OUT_OF_REACH_FOR - early) / TIME_SCALE as f64);
            for _ in 0..3000 {
                world.step(&mut human);
                if world.reliefs() > 0 {
                    return human.center_position();
                }
            }
            panic!("never relieved; brain: {:?}", human.brain_fields());
        };
        assert_eq!(goes_to(60.0), Point::new(13, 6), "a minute short of forgiven: the far one");
        assert_eq!(goes_to(0.0), Point::new(6, 6), "forgiven: the nearest again");
    }

    #[test]
    fn a_lone_toilet_given_up_on_is_still_tried_again_rather_than_never() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        prop_at(&mut map, "toilet", Point::new(6, 6));
        let mut world = World::new(map);
        let mut human = bursting_human(Point::new(5, 6), 95.0);
        human.brain_mut().memory_mut().out_of_reach_mut().remember(Point::new(6, 6), 0.0);
        for _ in 0..1000 {
            world.step(&mut human);
            if world.reliefs() > 0 {
                break;
            }
        }
        assert_eq!(world.reliefs(), 1, "the only toilet is still a toilet; brain: {:?}", human.brain_fields());
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
