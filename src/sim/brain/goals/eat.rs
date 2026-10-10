//! [`EatGoal`]: walk to the nearest fridge, take food out of it, eat it.
//!
//! The first goal that competes with another. Wandering is always wanted a
//! little; eating is wanted badly when hungry, and the whole architecture
//! exists for the moment this goal takes over from wandering and hands back
//! once the meal is done.
//!
//! It is three tasks, queued in order — [`MoveTo`](crate::sim::brain::tasks::MoveTo)
//! beside the fridge, [`TakeItem`](crate::sim::brain::tasks::TakeItem) food
//! out of it, [`ConsumeItem`](crate::sim::brain::tasks::ConsumeItem) the food —
//! and what this goal does is choose the fridge, queue them, and decide what a
//! failure means. Food going into a hand and hunger going down are the tasks'
//! and the body's doing, not this goal's.

use crate::map::Point;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;
use crate::sim::inventory::Inventory;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::{CLOSE_SECONDS, OPEN_SECONDS};
use super::{choose_nearest, nearest_in_reach, stand_beside, Seek, Stand};

/// A couple of world minutes at the fridge getting food out of it. Eating it
/// takes [`CHEW_SECONDS`](crate::sim::item::CHEW_SECONDS).
pub const TAKE_SECONDS: f32 = watched(2.0 * MINUTE);

/// Where a meal has got to, for a debugger — the queue is what the meal
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
    /// Opening the fridge's door.
    Opening,
    /// Standing at it, taking food out.
    Taking,
    /// Closing the fridge's door again.
    Closing,
    /// Food in hand, eating.
    Eating,
}

impl Stage {
    /// The stage the next task in the queue puts a meal at.
    fn of(next: Option<Task>) -> Stage {
        match next {
            Some(Task::ConsumeItem(task)) if task.item == ItemKind::Food => Stage::Eating,
            Some(Task::ConsumeItem(_)) => Stage::Clearing,
            // A wait is only ever queued in front of a walk.
            Some(Task::MoveTo(_) | Task::Wait(_)) => Stage::Walking,
            Some(Task::OpenFridge(_)) => Stage::Opening,
            Some(Task::TakeItem(_)) => Stage::Taking,
            Some(Task::CloseFridge(_)) => Stage::Closing,
            None | Some(Task::UseToilet(_) | Task::UseComputer(_) | Task::Sleep(_)) => Stage::Finding,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Stage::Finding => "finding",
            Stage::Clearing => "clearing its hand",
            Stage::Walking => "walking",
            Stage::Opening => "opening the fridge",
            Stage::Taking => "taking",
            Stage::Closing => "closing the fridge",
            Stage::Eating => "eating",
        }
    }
}

/// Eat something.
///
/// A fridge never runs out, so there is no depletion state to get wrong: the
/// fridge it chose and how many times a body has been in the way are the whole
/// of what there is to remember.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EatGoal {
    stage: Stage,
    /// Which place it is going to, and how the way there has gone.
    seek: Seek,
}

impl EatGoal {
    pub fn new() -> EatGoal {
        EatGoal::default()
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Queue the rest of the meal **from what is true now**, on the back of
    /// the queue: food already in hand is eaten; otherwise walk beside the
    /// fridge (unless already there), open it, take food, close it, eat it.
    /// Anything else in hand is finished first, since taking needs an empty
    /// hand.
    ///
    /// Worked out from the world rather than from a remembered stage, so a
    /// meal picked back up after an interruption starts from wherever it had
    /// really got to — the food a finished `TakeItem` put in hand is in hand
    /// whether or not this goal heard about it. `false` when there is nowhere
    /// to stand to use the fridge.
    ///
    /// **The plan resumes from the world here too.** Food already in hand,
    /// standing beside the fridge it came from, with that fridge still open —
    /// a unit called away between taking and closing, picked back up here —
    /// closes it before eating, rather than leaving it open for whoever
    /// happens by next.
    ///
    /// Finishing what is in hand rather than giving up on it is what keeps
    /// eating and drinking from deadlocking: water taken just before hunger
    /// took over would otherwise make every `TakeItem` fail, with nothing left
    /// in charge that wants to drink it.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let held = held(ctx);
        if held != Some(ItemKind::Food) {
            let Some(fridge) = self.seek.target else {
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
            let _ = ctx.tasks.push_back(Task::open_fridge(fridge, OPEN_SECONDS));
            let _ = ctx.tasks.push_back(Task::take(fridge, ItemKind::Food, TAKE_SECONDS));
            let _ = ctx.tasks.push_back(Task::close_fridge(fridge, CLOSE_SECONDS));
        } else if let Some(fridge) = self.seek.target
            && ctx.body.center_position().manhattan_distance(fridge) <= 1
            && ctx.think.fridges.is_open(fridge)
        {
            let _ = ctx.tasks.push_back(Task::close_fridge(fridge, CLOSE_SECONDS));
        }
        let _ = ctx
            .tasks
            .push_back(Task::consume(ItemKind::Food, ItemKind::Food.consume_seconds()));
        self.stage = Stage::of(ctx.tasks.front());
        true
    }

    fn forget(&mut self) {
        self.stage = Stage::Finding;
        self.seek.forget();
    }

    /// The meal is over: say so, and start the next one from scratch.
    fn eaten(&mut self, ctx: &GoalCtx<'_>) {
        let line = match self.seek.target {
            Some(fridge) => format!("{} ate at the fridge at {}, {}", ctx.body.uid(), fridge.x, fridge.y),
            None => format!("{} ate what it was carrying", ctx.body.uid()),
        };
        ctx.think.log.push(line);
        self.forget();
    }
}

fn held(ctx: &GoalCtx<'_>) -> Option<ItemKind> {
    ctx.inventory.and_then(Inventory::hand)
}

/// Whether `finished` is the last step of a meal — food eaten, not whatever
/// was cleared out of the hand first.
fn is_a_meal(finished: Option<Task>) -> bool {
    matches!(finished, Some(Task::ConsumeItem(task)) if task.item == ItemKind::Food)
}

impl GoalExecutor for EatGoal {
    fn goal(&self) -> GoalId {
        GoalId::Eat
    }

    /// The half-walked route is gone — the brain dropped it — but the fridge
    /// is not, so what is left of the meal goes back on the queue.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.seek.target.is_none() && held(ctx) != Some(ItemKind::Food) {
            return;
        }
        self.seek.steady();
        if !self.plan(ctx) {
            self.forget();
        }
    }

    /// A meal that ends is usually the reason this goal is put down: the last
    /// bite takes hunger under [`SATED`](crate::sim::brain::routines::SATED),
    /// and the routine lets go before this goal has been told the meal is
    /// over. So it is told here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() == TaskResult::Success && is_a_meal(ctx.finished) {
            self.eaten(ctx);
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() || ctx.inventory.is_none() {
            // A kind with no appetite, or no hands to eat with.
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there, or
                // no end to the crowd: remember it as out of reach and try
                // the next one (see `Seek`).
                if self.seek.setback(ctx, |ctx| nearest_in_reach(ctx, FeatureKind::Food)) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                // Nowhere left to try: give up on this visit rather than
                // retrying every tick.
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Taking or eating went wrong — standing in the wrong place,
                // a hand already full. Cheap to work out again, from scratch,
                // next tick.
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, finished) if is_a_meal(finished) => {
                self.eaten(ctx);
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, finished) => {
                if let Some(Task::MoveTo(_)) = finished {
                    self.seek.steady();
                }
                self.stage = Stage::of(ctx.tasks.front());
            }
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again.
        if held(ctx) != Some(ItemKind::Food) {
            let Some(fridge) = choose_nearest(ctx, FeatureKind::Food) else {
                return GoalProgress::Blocked;
            };
            self.seek.choose(fridge);
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
                match self.seek.target {
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
    use crate::sim::brain::memory::Memory;
    use crate::sim::brain::perception::Perception;
    use crate::sim::brain::task::Tasks;
    use crate::sim::brain::GoalId;
    use crate::sim::entity::Body;
    use crate::sim::item::MEAL;
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::{GameEntity, Human};

    /// Hungry, and nothing else pressing.
    fn hungry_human(cell: Point, hunger: f32) -> Human {
        needy_human(cell, hunger, 0.0, 0.0)
    }

    #[test]
    fn a_hungry_human_walks_to_the_fridge_and_stops_being_hungry() {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        let fridge = Point::new(11, 7);
        prop_at(&mut map, "fridge", fridge);
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(2, 2), 90.0);

        let mut ate = false;
        let mut went_for_food = false;
        for _ in 0..3000 {
            world.step(&mut human);
            went_for_food |= human.brain().top_goal() == GoalId::Eat;
            if world.log_contains("ate at the fridge") {
                ate = true;
                break;
            }
        }
        assert!(went_for_food, "hunger should have put eating in charge");
        assert!(ate, "never got a meal; the brain says {:?}", human.brain_fields());
        assert!(
            human.center_position().manhattan_distance(fridge) <= 1,
            "ate from {:?}, nowhere near the fridge at {fridge:?}",
            human.center_position()
        );
        let (x, y) = human.position();
        assert!(
            (x.fract() - 0.5).abs() < 1e-3 && (y.fract() - 0.5).abs() < 1e-3,
            "ate standing at {x}, {y} rather than in the middle of a cell"
        );
        // Loose: time passed on the walk there, and hunger rose while it did.
        assert!(human.stats().hunger() < 90.0 - MEAL / 2.0, "hunger {}", human.stats().hunger());
        assert_eq!(human.carried(), None, "the food was eaten, not kept");
        assert_eq!(world.drinks(), 0, "a meal is not a drink");
    }

    /// A fridge nobody wired up is a cupboard: the same walk as above, as
    /// the map is built, never ends in a meal — and plugged in, it does.
    #[test]
    fn a_fridge_with_no_power_feeds_nobody_until_it_is_wired_up() {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "fridge", Point::new(11, 7));
        let run = |map: Map| {
            let mut world = World::as_built(map);
            let mut human = hungry_human(Point::new(2, 2), 90.0);
            for _ in 0..3000 {
                world.step(&mut human);
            }
            world.meals()
        };
        assert_eq!(run(map.clone()), 0, "ate from a fridge with no power");

        crate::map::utilities::serve_everything(&mut map, Point::new(0, 9), Point::new(13, 9));
        assert!(run(map) > 0, "never ate once the fridge had power");
    }

    #[test]
    fn a_hungry_human_keeps_eating_until_it_is_sated_and_then_wanders_again() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        prop_at(&mut map, "fridge", Point::new(5, 5));
        let mut world = World::new(map);
        // One meal is not enough to come down from here.
        let mut human = hungry_human(Point::new(1, 1), 100.0);

        let mut wandering_again = false;
        for _ in 0..4000 {
            world.step(&mut human);
            if world.meals() >= 2 && human.brain().top_goal() == GoalId::Wander {
                wandering_again = true;
                break;
            }
        }
        assert!(wandering_again, "meals: {}, brain: {:?}", world.meals(), human.brain_fields());
    }

    #[test]
    fn a_human_already_holding_food_eats_it_without_going_to_the_fridge() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        prop_at(&mut map, "fridge", Point::new(10, 10));
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(2, 2), 90.0);
        human.set_carried(Some(ItemKind::Food));
        let start = human.position();

        for _ in 0..600 {
            world.step(&mut human);
            if human.carried().is_none() {
                break;
            }
        }
        assert_eq!(human.carried(), None, "the food should have been eaten");
        assert_eq!(human.position(), start, "and eaten on the spot");
        assert!(human.stats().hunger() < 90.0 - MEAL / 2.0, "hunger {}", human.stats().hunger());
    }

    #[test]
    fn a_hungry_human_goes_round_the_furniture_and_eats_from_the_side_of_the_fridge_it_can_reach() {
        // A crate and the fridge sit in a row between walls, the crate on the
        // near side: straight through them is the short way to the fridge's
        // far side, (5, 3), and the only way that is allowed is round.
        let mut map = Map::new(Size::new(7, 7), FLOOR);
        let (fridge, crate_) = (Point::new(4, 3), Point::new(3, 3));
        prop_at(&mut map, "fridge", fridge);
        prop_at(&mut map, "crate", crate_);
        for x in 3..6 {
            map.set_terrain(Point::new(x, 2), WALL);
            map.set_terrain(Point::new(x, 4), WALL);
        }
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(1, 3), 90.0);

        for _ in 0..3000 {
            world.step(&mut human);
            let here = human.center_position();
            assert!(here != fridge && here != crate_, "walked into the furniture at {here:?}");
            if world.meals() > 0 {
                break;
            }
        }
        assert_eq!(world.meals(), 1, "brain: {:?}", human.brain_fields());
        assert_eq!(human.center_position(), Point::new(5, 3), "the one side of it there is");
    }

    #[test]
    fn a_hungry_human_holding_water_drinks_it_before_taking_food() {
        // Taking needs an empty hand. Water taken just before hunger took over
        // would otherwise fail every meal, and nothing thirsty enough is left
        // in charge to drink it.
        let mut map = Map::new(Size::new(8, 8), FLOOR);
        prop_at(&mut map, "fridge", Point::new(4, 4));
        let mut world = World::new(map);
        let mut human = needy_human(Point::new(3, 4), 90.0, 40.0, 0.0);
        human.set_carried(Some(ItemKind::Water));

        for _ in 0..1000 {
            world.step(&mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Drink, "not thirsty enough to be drinking");
            if world.log_contains("ate at the fridge") {
                break;
            }
        }
        assert!(world.log_contains("ate at the fridge"), "brain: {:?}", human.brain_fields());
        assert_eq!(human.carried(), None);
        assert!(human.stats().thirst() < 40.0 - 20.0, "the water was drunk on the way");
    }

    /// The reason [`Inventory`] never refuses a hand: a unit loaded to both
    /// its limits must still be able to pick food up and eat it. If the limits
    /// governed the hand as well, a full unit would fail every `TakeItem`
    /// forever and starve standing next to a fridge, with nothing in the brain
    /// able to say why.
    #[test]
    fn a_human_loaded_to_its_limits_can_still_pick_food_up_and_eat_it() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        prop_at(&mut map, "fridge", Point::new(5, 5));
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(2, 2), 90.0);
        while human.inventory_mut().stow(ItemKind::Food) {}
        let laden = human.inventory().count(ItemKind::Food);
        assert!(laden > 0 && !human.inventory().room_for(ItemKind::Food), "should be full");

        for _ in 0..3000 {
            world.step(&mut human);
            if world.meals() > 0 {
                break;
            }
        }
        assert_eq!(world.meals(), 1, "brain: {:?}", human.brain_fields());
        // It fetched a fresh one rather than eating out of its own pockets:
        // no goal knows how to unstow anything yet, and the hand is the only
        // slot the brain uses. The stowed food is untouched.
        assert_eq!(human.inventory().count(ItemKind::Food), laden);
        assert_eq!(human.carried(), None, "the one it fetched was eaten");
    }

    #[test]
    fn a_hungry_human_with_no_fridge_in_the_world_goes_back_to_wandering() {
        let map = Map::new(Size::new(12, 12), FLOOR);
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(6, 6), 95.0);
        let start = human.position();

        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Eat) > 0, "eating should be held off");
        assert_ne!(human.position(), start, "and it should be walking about meanwhile");
    }

    #[test]
    fn a_hungry_human_opens_the_fridge_before_taking_and_closes_it_after() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let fridge = Point::new(5, 5);
        prop_at(&mut map, "fridge", fridge);
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(2, 2), 90.0);

        for _ in 0..3000 {
            world.step(&mut human);
            if world.log_contains("ate at the fridge") {
                break;
            }
        }
        assert!(world.log_contains("ate at the fridge"), "brain: {:?}", human.brain_fields());
        let position = |needle: &str| world.lines().iter().position(|line| line.contains(needle));
        let (opened, closed, ate) = (
            position("opened the fridge").expect("should have opened it"),
            position("closed the fridge").expect("should have closed it"),
            position("ate at the fridge").expect("should have eaten"),
        );
        assert!(opened < closed && closed < ate, "wrong order: opened {opened}, closed {closed}, ate {ate}");
        assert!(!world.fridges.is_open(fridge), "should have left the fridge closed");
    }

    #[test]
    fn the_fridge_warms_while_the_meal_is_taken_out_and_cools_back_down_afterwards() {
        // A human this hungry eats twice (the sibling test above), and keeps
        // getting hungry again as world time passes, so the fridge may cycle
        // open and shut more than once inside any tick budget generous enough
        // to prove it cools down at all. What is asserted is the *sequence* —
        // warmed, then cooled, in that order — not a temperature at a fixed
        // tick, which a freshly reopened door could fail for reasons that
        // have nothing to do with cooling.
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let fridge = Point::new(5, 5);
        prop_at(&mut map, "fridge", fridge);
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(2, 2), 90.0);

        let mut warmed = false;
        let mut cooled = false;
        for _ in 0..40_000 {
            world.step(&mut human);
            let temperature = world.fridges.temperature(fridge);
            warmed |= temperature > crate::sim::fridge::COLDEST + 0.5;
            if warmed && !world.fridges.is_open(fridge) && temperature < crate::sim::fridge::COLDEST + 0.5 {
                cooled = true;
                break;
            }
        }
        assert!(warmed, "the fridge should have warmed up while its door was open");
        assert!(cooled, "the fridge never cooled back down after warming up");
    }

    /// `GoalCtx` fields are all `pub`, so this drives `EatGoal::prioritized`
    /// directly rather than through a full simulation: the scenario this
    /// covers — put down between taking and closing, picked back up **still
    /// standing beside the fridge it left open** — is what a queue clearing
    /// without a walk elsewhere looks like, which a goal handover in a real
    /// world does not reliably reproduce (the goal that interrupts a meal
    /// almost always walks the unit away from the fridge first).
    #[test]
    fn a_human_holding_food_beside_a_fridge_it_left_open_closes_it_before_eating() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let fridge = Point::new(5, 5);
        prop_at(&mut map, "fridge", fridge);
        let mut world = World::new(map);
        assert!(world.fridges.set_open(fridge, true));

        let mut goal = EatGoal {
            stage: Stage::Finding,
            seek: Seek::at(fridge),
        };
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(4, 5));
        let mut memory = Memory::default();
        let mut tasks = Tasks::new();
        let mut inventory = Inventory::human();
        let _ = inventory.set_hand(Some(ItemKind::Food));
        let perception = Perception::default();
        let think = world.ctx();
        let mut ctx = GoalCtx {
            think: &think,
            body: &body,
            perception: &perception,
            biology: None,
            memory: &mut memory,
            tasks: &mut tasks,
            inventory: Some(&inventory),
            blocked_by: None,
            finished: None,
        };

        goal.prioritized(&mut ctx);

        assert_eq!(tasks.front(), Some(Task::close_fridge(fridge, CLOSE_SECONDS)));
        assert_eq!(tasks.back(), Some(Task::consume(ItemKind::Food, ItemKind::Food.consume_seconds())));
        assert_eq!(tasks.len(), 2);
    }

    /// The same resume, but standing somewhere else by the time it is picked
    /// back up — walked off to the toilet and back, say. Nothing closes the
    /// fridge on this unit's behalf; it stays open and warming for whoever
    /// gets there next, exactly as CLAUDE.md says it should.
    #[test]
    fn a_human_holding_food_far_from_the_fridge_it_left_open_does_not_close_it() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let fridge = Point::new(5, 5);
        prop_at(&mut map, "fridge", fridge);
        let mut world = World::new(map);
        assert!(world.fridges.set_open(fridge, true));

        let mut goal = EatGoal {
            stage: Stage::Finding,
            seek: Seek::at(fridge),
        };
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut memory = Memory::default();
        let mut tasks = Tasks::new();
        let mut inventory = Inventory::human();
        let _ = inventory.set_hand(Some(ItemKind::Food));
        let perception = Perception::default();
        let think = world.ctx();
        let mut ctx = GoalCtx {
            think: &think,
            body: &body,
            perception: &perception,
            biology: None,
            memory: &mut memory,
            tasks: &mut tasks,
            inventory: Some(&inventory),
            blocked_by: None,
            finished: None,
        };

        goal.prioritized(&mut ctx);

        assert_eq!(tasks.len(), 1, "only the meal itself, no close queued");
        assert_eq!(tasks.front(), Some(Task::consume(ItemKind::Food, ItemKind::Food.consume_seconds())));
        assert!(world.fridges.is_open(fridge), "left open, for somebody else to close");
    }

    #[test]
    fn a_fridge_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        // The fridge sits in a sealed room: the route to it is the expensive
        // miss, and the goal has to be held off for it.
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for i in 7..12 {
            map.set_terrain(Point::new(i, 7), WALL);
            map.set_terrain(Point::new(7, i), WALL);
        }
        prop_at(&mut map, "fridge", Point::new(10, 10));
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(2, 2), 95.0);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..1000 {
            world.step(&mut human);
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert!(world.meals() == 0);
        // Wandering asks for a route every few seconds; a fridge retried
        // every tick would be a thousand.
        assert!(routes < 100, "{routes} routes in 1000 ticks");
    }

    #[test]
    fn a_walled_off_fridge_is_passed_over_on_the_spot_for_one_that_can_be_reached() {
        // The nearer fridge, as the crow flies, is sealed in a room. The
        // same mechanism as the toilet's (`Seek`), so a fridge it could not
        // get to is remembered, and the next one is gone for at once.
        let mut map = Map::new(Size::new(16, 12), FLOOR);
        for i in 0..5 {
            map.set_terrain(Point::new(i, 4), WALL);
            map.set_terrain(Point::new(4, i), WALL);
        }
        prop_at(&mut map, "fridge", Point::new(1, 1));
        prop_at(&mut map, "fridge", Point::new(14, 10));
        let mut world = World::new(map);
        let mut human = hungry_human(Point::new(5, 5), 95.0);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        let mut held_off = false;
        for _ in 0..3000 {
            world.step(&mut human);
            held_off |= human.brain().goals().cooldown(GoalId::Eat) > 0;
            if world.meals() > 0 {
                break;
            }
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert!(world.log_contains("gave up on reaching 1, 1"), "lines: {:?}", world.lines());
        assert!(world.log_contains("ate at the fridge at 14, 10"), "brain: {:?}", human.brain_fields());
        assert!(!held_off, "the sealed fridge should have been swapped for the other on the spot");
        assert!(routes < 20, "{routes} routes for one rerouted meal");
    }

    #[test]
    fn a_fridge_given_up_on_for_food_is_not_gone_back_to_for_a_drink() {
        // Memory is of places, not of what they were wanted for: a fridge is
        // food and water, and it is just as out of reach for either.
        let mut map = Map::new(Size::new(16, 12), FLOOR);
        prop_at(&mut map, "fridge", Point::new(6, 6));
        prop_at(&mut map, "fridge", Point::new(13, 6));
        let mut world = World::new(map);
        let mut human = needy_human(Point::new(5, 6), 0.0, 95.0, 0.0);
        human.brain_mut().memory_mut().out_of_reach_mut().remember(Point::new(6, 6), 0.0);
        for _ in 0..3000 {
            world.step(&mut human);
            if world.drinks() > 0 {
                break;
            }
        }
        assert!(world.log_contains("drank at the fridge at 13, 6"), "brain: {:?}", human.brain_fields());
    }
}
