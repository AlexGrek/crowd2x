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
//! doing, not this goal's.

use crate::map::Point;
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{PATIENCE, WAIT_FOR_A_GAP};

/// Seconds spent at the fridge getting food out of it.
pub const TAKE_SECONDS: f32 = 1.0;

/// Seconds spent eating what was taken.
pub const CHEW_SECONDS: f32 = 2.0;

/// Where a meal has got to, for a debugger — the queue is what the meal
/// actually is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Stage {
    /// Deciding which fridge.
    #[default]
    Finding,
    /// On the way to it.
    Walking,
    /// Standing at it, taking food out.
    Taking,
    /// Food in hand, eating.
    Eating,
}

impl Stage {
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Finding => "finding",
            Stage::Walking => "walking",
            Stage::Taking => "taking",
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
    /// The fridge, by cell.
    target: Option<Point>,
    /// Walks to the fridge blocked by a body so far, in a row.
    retries: u8,
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
    /// fridge (unless already there), take food, eat it.
    ///
    /// Worked out from the world rather than from a remembered stage, so a
    /// meal picked back up after an interruption starts from wherever it had
    /// really got to — the food a finished `TakeItem` put in hand is in hand
    /// whether or not this goal heard about it. `false` when there is nowhere
    /// to stand to use the fridge.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        if holding_food(ctx) {
            let _ = ctx.tasks.push_back(Task::consume(ItemKind::Food, CHEW_SECONDS));
            self.stage = Stage::Eating;
            return true;
        }
        let Some(fridge) = self.target else {
            self.stage = Stage::Finding;
            return false;
        };
        match stand_beside(ctx, fridge) {
            Stand::Nowhere => {
                self.stage = Stage::Finding;
                return false;
            }
            Stand::Here => self.stage = Stage::Taking,
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_to(cell));
                self.stage = Stage::Walking;
            }
        }
        let _ = ctx.tasks.push_back(Task::take(fridge, ItemKind::Food, TAKE_SECONDS));
        let _ = ctx.tasks.push_back(Task::consume(ItemKind::Food, CHEW_SECONDS));
        true
    }

    fn forget(&mut self) {
        self.stage = Stage::Finding;
        self.target = None;
        self.retries = 0;
    }

    /// The meal is over: say so, and start the next one from scratch.
    fn eaten(&mut self, ctx: &GoalCtx<'_>) {
        let line = match self.target {
            Some(fridge) => format!("{} ate at the fridge at {}, {}", ctx.body.uid(), fridge.x, fridge.y),
            None => format!("{} ate what it was carrying", ctx.body.uid()),
        };
        ctx.think.log.push(line);
        self.forget();
    }
}

fn holding_food(ctx: &GoalCtx<'_>) -> bool {
    ctx.carried == Some(&Some(ItemKind::Food))
}

/// Where to stand to use a fridge.
enum Stand {
    /// Already close enough.
    Here,
    At(Point),
    /// Nowhere passable touches it.
    Nowhere,
}

/// The cell to use `fridge` from: beside it, passable, preferably free, and
/// nearest.
///
/// Four cells and the fridge's own — props do not block terrain, so standing
/// in the fridge's cell is allowed as a last resort. The crowd is read for
/// those five cells and nothing else: a human choosing the free side of a
/// fridge is a lookup, not a scan.
fn stand_beside(ctx: &GoalCtx<'_>, fridge: Point) -> Stand {
    let here = ctx.body.center_position();
    if here.manhattan_distance(fridge) <= 1 {
        return Stand::Here;
    }
    let uid = ctx.body.uid();
    Point::CARDINALS
        .iter()
        .map(|&step| fridge + step)
        .chain(std::iter::once(fridge))
        .filter(|&cell| ctx.think.is_passable(cell))
        .min_by_key(|&cell| {
            (
                !ctx.think.occupancy.is_free_for(cell, uid),
                cell == fridge,
                here.manhattan_distance(cell),
                cell,
            )
        })
        .map_or(Stand::Nowhere, Stand::At)
}

impl GoalExecutor for EatGoal {
    fn goal(&self) -> GoalId {
        GoalId::Eat
    }

    /// The half-walked route is gone — the brain dropped it — but the fridge
    /// is not, so what is left of the meal goes back on the queue.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.target.is_none() && !holding_food(ctx) {
            return;
        }
        self.retries = 0;
        if !self.plan(ctx) {
            self.forget();
        }
    }

    /// A meal that ends is usually the reason this goal is put down: the last
    /// bite takes hunger under [`SATED`](crate::sim::brain::routines::SATED),
    /// and the routine lets go before this goal has been told the meal is
    /// over. So it is told here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() == TaskResult::Success && matches!(ctx.finished, Some(Task::ConsumeItem(_))) {
            self.eaten(ctx);
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.stats.is_none() || ctx.carried.is_none() {
            // A kind with no appetite, or no hands to eat with.
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
                // Taking or eating went wrong — standing in the wrong place,
                // a hand already full. Cheap to work out again, from scratch,
                // next tick.
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => {
                self.stage = Stage::Taking;
                self.retries = 0;
            }
            (TaskResult::Success, Some(Task::TakeItem(_))) => self.stage = Stage::Eating,
            (TaskResult::Success, Some(Task::ConsumeItem(_))) => {
                self.eaten(ctx);
                return GoalProgress::Achieved;
            }
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again.
        if !holding_food(ctx) {
            let here = ctx.body.center_position();
            let Some(fridge) = ctx.think.features.nearest(FeatureKind::Food, here) else {
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
    use crate::map::{Map, Object, ObjectKind, ObjectLayer, Size, FLOOR, PIXELS_PER_CELL, WALL};
    use crate::sim::brain::GoalId;
    use crate::sim::item::MEAL;
    use crate::sim::testing::World;
    use crate::sim::{EntityType, GameEntity, Human};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    fn fridge_at(map: &mut Map, cell: Point) {
        map.add_object(
            ObjectLayer::Props,
            Object {
                at: Point::new(
                    cell.x * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
                    cell.y * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
                ),
                kind: ObjectKind::new("fridge"),
            },
        );
    }

    fn hungry_human(cell: Point, hunger: f32) -> Human {
        let mut rng = SmallRng::seed_from_u64(3);
        let mut human = Human::new(crate::sim::Uid::new(EntityType::Human, 77), cell, &mut rng);
        human.set_hunger(hunger);
        human
    }

    #[test]
    fn a_hungry_human_walks_to_the_fridge_and_stops_being_hungry() {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        let fridge = Point::new(11, 7);
        fridge_at(&mut map, fridge);
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
    }

    #[test]
    fn a_hungry_human_keeps_eating_until_it_is_sated_and_then_wanders_again() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        fridge_at(&mut map, Point::new(5, 5));
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
        fridge_at(&mut map, Point::new(10, 10));
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
    fn a_fridge_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        // The fridge sits in a sealed room: the route to it is the expensive
        // miss, and the goal has to be held off for it.
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for i in 7..12 {
            map.set_terrain(Point::new(i, 7), WALL);
            map.set_terrain(Point::new(7, i), WALL);
        }
        fridge_at(&mut map, Point::new(10, 10));
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
}
