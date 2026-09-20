//! [`DrinkGoal`]: walk to the nearest fridge, take a drink out of it, drink
//! it.
//!
//! Three tasks, queued in order — [`MoveTo`](crate::sim::brain::tasks::MoveTo)
//! beside the fridge, [`TakeItem`](crate::sim::brain::tasks::TakeItem) water
//! out of it, [`ConsumeItem`](crate::sim::brain::tasks::ConsumeItem) the water.
//! The same steps as a meal, quicker, and its own goal rather than a meal with
//! different numbers in: what drinking means to a body is already not what
//! eating means (a drink fills the bladder), and the two are free to grow apart
//! from here.

use crate::map::Point;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;
use crate::sim::inventory::Inventory;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Stand, PATIENCE, WAIT_FOR_A_GAP};

/// A world minute at the fridge getting a drink out of it: half as long as
/// food. Drinking it takes [`SIP_SECONDS`](crate::sim::item::SIP_SECONDS).
pub const POUR_SECONDS: f32 = watched(1.0 * MINUTE);

/// Where a drink has got to, for a debugger — the queue is what the drink
/// actually is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Stage {
    /// Deciding which fridge.
    #[default]
    Finding,
    /// Using up something else it was holding, which is in the way of taking.
    Clearing,
    /// On the way to it.
    Walking,
    /// Standing at it, taking a drink out.
    Taking,
    /// Water in hand, drinking.
    Drinking,
}

impl Stage {
    /// The stage the next task in the queue puts a drink at.
    fn of(next: Option<Task>) -> Stage {
        match next {
            Some(Task::ConsumeItem(task)) if task.item == ItemKind::Water => Stage::Drinking,
            Some(Task::ConsumeItem(_)) => Stage::Clearing,
            // A wait is only ever queued in front of a walk.
            Some(Task::MoveTo(_) | Task::Wait(_)) => Stage::Walking,
            Some(Task::TakeItem(_)) => Stage::Taking,
            None | Some(Task::UseToilet(_) | Task::UseComputer(_)) => Stage::Finding,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Stage::Finding => "finding",
            Stage::Clearing => "clearing its hand",
            Stage::Walking => "walking",
            Stage::Taking => "taking",
            Stage::Drinking => "drinking",
        }
    }
}

/// Drink something.
///
/// Like a meal, a fridge never runs out of drinks: the fridge it chose and how
/// many times a body has been in the way are all there is to remember.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DrinkGoal {
    stage: Stage,
    /// The fridge, by cell.
    target: Option<Point>,
    /// Walks to the fridge blocked by a body so far, in a row.
    retries: u8,
}

impl DrinkGoal {
    pub fn new() -> DrinkGoal {
        DrinkGoal::default()
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Queue the rest of the drink **from what is true now**, on the back of
    /// the queue: water already in hand is drunk; otherwise walk beside the
    /// fridge (unless already there), take water, drink it. Anything else in
    /// hand is finished first, since taking needs an empty hand — food taken
    /// just before thirst took over would otherwise make every drink fail,
    /// with nothing hungry enough left in charge to eat it. `false` when there
    /// is nowhere to stand to use the fridge.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let held = held(ctx);
        if held != Some(ItemKind::Water) {
            let Some(fridge) = self.target else {
                return false;
            };
            let stand = stand_beside(ctx, fridge);
            if let Stand::Nowhere = stand {
                return false;
            }
            if let Some(other) = held {
                let _ = ctx.tasks.push_back(Task::consume(other, other.consume_seconds()));
            }
            if let Stand::At(cell) = stand {
                let _ = ctx.tasks.push_back(Task::move_to(cell));
            }
            let _ = ctx.tasks.push_back(Task::take(fridge, ItemKind::Water, POUR_SECONDS));
        }
        let _ = ctx
            .tasks
            .push_back(Task::consume(ItemKind::Water, ItemKind::Water.consume_seconds()));
        self.stage = Stage::of(ctx.tasks.front());
        true
    }

    fn forget(&mut self) {
        self.stage = Stage::Finding;
        self.target = None;
        self.retries = 0;
    }

    /// The drink is over: say so, and start the next one from scratch.
    fn drunk(&mut self, ctx: &GoalCtx<'_>) {
        let line = match self.target {
            Some(fridge) => format!("{} drank at the fridge at {}, {}", ctx.body.uid(), fridge.x, fridge.y),
            None => format!("{} drank what it was carrying", ctx.body.uid()),
        };
        ctx.think.log.push(line);
        self.forget();
    }
}

fn held(ctx: &GoalCtx<'_>) -> Option<ItemKind> {
    ctx.inventory.and_then(Inventory::hand)
}

/// Whether `finished` is the last step of a drink — water drunk, not whatever
/// was cleared out of the hand first.
fn is_a_drink(finished: Option<Task>) -> bool {
    matches!(finished, Some(Task::ConsumeItem(task)) if task.item == ItemKind::Water)
}

impl GoalExecutor for DrinkGoal {
    fn goal(&self) -> GoalId {
        GoalId::Drink
    }

    /// The half-walked route is gone — the brain dropped it — but the fridge
    /// is not, so what is left of the drink goes back on the queue.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.target.is_none() && held(ctx) != Some(ItemKind::Water) {
            return;
        }
        self.retries = 0;
        if !self.plan(ctx) {
            self.forget();
        }
    }

    /// The last sip usually takes thirst under
    /// [`QUENCHED`](crate::sim::brain::routines::QUENCHED) and the routine
    /// lets go before this goal has been told the drink is over, so it is told
    /// here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() == TaskResult::Success && is_a_drink(ctx.finished) {
            self.drunk(ctx);
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() || ctx.inventory.is_none() {
            // A kind with no thirst, or no hands to drink with.
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                if ctx.blocked_by.is_some() && self.retries < PATIENCE && self.target.is_some() {
                    // Somebody in the way of the fridge: bodies move. Give them
                    // a moment, then set off again.
                    self.retries += 1;
                    let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                    if self.plan(ctx) {
                        return GoalProgress::Working;
                    }
                }
                // No way to the fridge, or no end to the crowd round it.
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Taking or drinking went wrong. Cheap to work out again, from
                // scratch, next tick.
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, finished) if is_a_drink(finished) => {
                self.drunk(ctx);
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, finished) => {
                if let Some(Task::MoveTo(_)) = finished {
                    self.retries = 0;
                }
                self.stage = Stage::of(ctx.tasks.front());
            }
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again.
        if held(ctx) != Some(ItemKind::Water) {
            let here = ctx.body.center_position();
            let Some(fridge) = ctx.think.features.nearest(FeatureKind::Water, here) else {
                return GoalProgress::Blocked;
            };
            self.target = Some(fridge);
        }
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("stage", self.stage.name().to_string()),
            (
                "fridge",
                match self.target {
                    Some(cell) => format!("{}, {}", cell.x, cell.y),
                    None => "none".to_string(),
                },
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR, WALL};
    use crate::sim::brain::routines::{QUENCHED, SATED};
    use crate::sim::brain::GoalId;
    use crate::sim::item::{DRINK, MEAL};
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::{GameEntity, Human};

    /// Thirsty, and nothing else pressing.
    fn thirsty_human(cell: Point, thirst: f32) -> Human {
        needy_human(cell, 0.0, thirst, 0.0)
    }

    #[test]
    fn a_thirsty_human_walks_to_the_fridge_and_stops_being_thirsty() {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        let fridge = Point::new(11, 7);
        prop_at(&mut map, "fridge", fridge);
        let mut world = World::new(map);
        let mut human = thirsty_human(Point::new(2, 2), 90.0);

        let mut drank = false;
        let mut went_for_a_drink = false;
        for _ in 0..3000 {
            world.step(&mut human);
            went_for_a_drink |= human.brain().top_goal() == GoalId::Drink;
            if world.log_contains("drank at the fridge") {
                drank = true;
                break;
            }
        }
        assert!(went_for_a_drink, "thirst should have put drinking in charge");
        assert!(drank, "never got a drink; the brain says {:?}", human.brain_fields());
        assert!(human.center_position().manhattan_distance(fridge) <= 1);
        assert!(human.stats().thirst() < 90.0 - DRINK / 2.0, "thirst {}", human.stats().thirst());
        assert_eq!(world.meals(), 0, "a drink is not a meal");
        assert_eq!(human.carried(), None, "the water was drunk, not kept");
    }

    #[test]
    fn a_drink_takes_less_time_at_the_fridge_than_a_meal() {
        // Both start beside the fridge, so the count is taking and using up
        // and nothing else.
        let ticks_to = |mut human: Human, line: &str| {
            let mut map = Map::new(Size::new(8, 8), FLOOR);
            prop_at(&mut map, "fridge", Point::new(4, 4));
            let mut world = World::new(map);
            for tick in 1..2000 {
                world.step(&mut human);
                if world.log_contains(line) {
                    return tick;
                }
            }
            panic!("never {line}");
        };
        let meal = ticks_to(needy_human(Point::new(3, 4), 90.0, 0.0, 0.0), "ate at the fridge");
        let drink = ticks_to(thirsty_human(Point::new(3, 4), 90.0), "drank at the fridge");
        assert!(drink < meal, "a drink took {drink} ticks and a meal {meal}");
    }

    #[test]
    fn a_thirsty_human_holding_food_eats_it_before_taking_a_drink() {
        let mut map = Map::new(Size::new(8, 8), FLOOR);
        prop_at(&mut map, "fridge", Point::new(4, 4));
        let mut world = World::new(map);
        let mut human = needy_human(Point::new(3, 4), 40.0, 90.0, 0.0);
        human.set_carried(Some(ItemKind::Food));

        for _ in 0..1000 {
            world.step(&mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Eat, "not hungry enough to be eating");
            if world.log_contains("drank at the fridge") {
                break;
            }
        }
        assert!(world.log_contains("drank at the fridge"), "brain: {:?}", human.brain_fields());
        assert_eq!(human.carried(), None);
        assert!(human.stats().hunger() < 40.0 - MEAL / 2.0, "the food was eaten on the way");
    }

    #[test]
    fn a_human_both_hungry_and_thirsty_sees_to_the_worse_first_then_both_without_turning_round() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        prop_at(&mut map, "fridge", Point::new(9, 9));
        let mut world = World::new(map);
        let mut human = needy_human(Point::new(2, 2), 90.0, 95.0, 0.0);
        // The toilet is not what this is about, and there is none here.
        human.biology_mut().unwrap().set_running(crate::sim::ProcessId::Bladder, false);

        let mut first = None;
        let mut was = human.brain().top_goal();
        let mut busy = false;
        for tick in 0..6000 {
            world.step(&mut human);
            let now = human.brain().top_goal();
            if first.is_none() && matches!(now, GoalId::Eat | GoalId::Drink) {
                first = Some(now);
            }
            assert!(
                now == was || !busy,
                "tick {tick}: {was:?} was put down for {now:?} in the middle of {:?}",
                human.brain().action()
            );
            was = now;
            busy = human.brain().current_task().is_some() && was != GoalId::Wander;
            if world.meals() > 0
                && world.drinks() > 0
                && human.stats().hunger() <= SATED
                && human.stats().thirst() <= QUENCHED
                && now == GoalId::Wander
            {
                break;
            }
        }
        assert_eq!(first, Some(GoalId::Drink), "thirst was the worse of the two");
        assert!(world.meals() > 0 && world.drinks() > 0, "brain: {:?}", human.brain_fields());
        assert_eq!(human.brain().top_goal(), GoalId::Wander, "both seen to: {:?}", human.stats());
    }

    #[test]
    fn a_thirsty_human_with_no_fridge_in_the_world_goes_back_to_wandering() {
        let map = Map::new(Size::new(12, 12), FLOOR);
        let mut world = World::new(map);
        let mut human = thirsty_human(Point::new(6, 6), 95.0);
        let start = human.position();

        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Drink) > 0, "drinking should be held off");
        assert_ne!(human.position(), start, "and it should be walking about meanwhile");
    }

    #[test]
    fn a_fridge_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for i in 7..12 {
            map.set_terrain(Point::new(i, 7), WALL);
            map.set_terrain(Point::new(7, i), WALL);
        }
        prop_at(&mut map, "fridge", Point::new(10, 10));
        let mut world = World::new(map);
        let mut human = thirsty_human(Point::new(2, 2), 95.0);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..1000 {
            world.step(&mut human);
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert!(world.drinks() == 0);
        assert!(routes < 100, "{routes} routes in 1000 ticks");
    }
}
