//! [`EatGoal`]: walk to the nearest fridge, take food out of it, eat it.
//!
//! The first goal that competes with another. Wandering is always wanted a
//! little; eating is wanted badly when hungry, and the whole architecture
//! exists for the moment this goal takes over from wandering and hands back
//! once the meal is done.

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

/// How much hunger one meal takes away.
pub const MEAL: f32 = 60.0;

/// Where a meal has got to — and, since it survives being put down, where it
/// picks up again.
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
/// stages are the whole of what there is to remember, and the fridge it chose.
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

    /// Queue what is left of the meal from `stage` on, and note the stage it
    /// actually starts at.
    ///
    /// **Built back to front, on the front of the queue.** Eating is the
    /// point; food in hand is its prerequisite; standing at the fridge is
    /// that one's. Each is pushed in front of what it makes possible, which is
    /// the same move as resuming — so there is one way to queue a meal,
    /// whether it is being started or picked back up.
    fn queue_from(&mut self, ctx: &mut GoalCtx<'_>, stage: Stage) -> bool {
        let Some(fridge) = self.target else {
            self.stage = Stage::Finding;
            return false;
        };
        let _ = ctx.tasks.push_front(Task::Wait {
            seconds: CHEW_SECONDS,
        });
        self.stage = Stage::Eating;
        if stage == Stage::Eating {
            return true;
        }

        let _ = ctx.tasks.push_front(Task::Interact {
            cell: fridge,
            seconds: TAKE_SECONDS,
        });
        self.stage = Stage::Taking;
        if stage == Stage::Taking {
            return true;
        }

        match stand_beside(ctx, fridge) {
            Stand::Here => true,
            Stand::At(cell) => {
                let _ = ctx.tasks.push_front(Task::MoveTo { cell });
                self.stage = Stage::Walking;
                true
            }
            Stand::Nowhere => {
                ctx.tasks.clear();
                self.stage = Stage::Finding;
                false
            }
        }
    }

    fn forget(&mut self) {
        self.stage = Stage::Finding;
        self.target = None;
        self.retries = 0;
    }

    fn carrying_food(ctx: &GoalCtx<'_>) -> bool {
        matches!(ctx.carried.as_deref(), Some(Some(ItemKind::Food)))
    }
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

    /// The half-walked route is gone — the brain dropped it — but the stage and
    /// the fridge are not, so what is left of the meal goes back on the queue.
    /// Food already in hand stays in hand.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.stage != Stage::Finding {
            self.retries = 0;
            // Walking or not, where to stand is decided again from here.
            let from = match self.stage {
                Stage::Taking => Stage::Walking,
                stage => stage,
            };
            if !self.queue_from(ctx, from) {
                self.forget();
            }
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.stats.is_none() || ctx.carried.is_none() {
            // A kind with no appetite, or no hands to eat with.
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Failed, _) => {
                ctx.tasks.clear();
                match ctx.blocked_by {
                    // Somebody in the way of the fridge: bodies move.
                    Some(_) if self.retries < PATIENCE && self.target.is_some() => {
                        self.retries += 1;
                        let _ = self.queue_from(ctx, Stage::Walking);
                        let _ = ctx.tasks.push_front(Task::Wait {
                            seconds: WAIT_FOR_A_GAP,
                        });
                        return GoalProgress::Working;
                    }
                    // No way to the fridge, or no end to the crowd round it.
                    _ => {
                        let interacting = self.stage == Stage::Taking;
                        self.forget();
                        // An interaction that failed was only ever a matter of
                        // standing in the wrong place — cheap to try again.
                        return if interacting {
                            GoalProgress::Working
                        } else {
                            GoalProgress::Blocked
                        };
                    }
                }
            }
            (TaskResult::Success, Some(Task::MoveTo { .. })) => {
                self.stage = Stage::Taking;
                self.retries = 0;
            }
            (TaskResult::Success, Some(Task::Interact { .. })) => {
                if let Some(hand) = ctx.carried.as_deref_mut() {
                    *hand = Some(ItemKind::Food);
                }
                self.stage = Stage::Eating;
            }
            (TaskResult::Success, Some(Task::Wait { .. })) if self.stage == Stage::Eating => {
                if let Some(hand) = ctx.carried.as_deref_mut() {
                    hand.take();
                }
                if let Some(stats) = ctx.stats.as_deref_mut() {
                    stats.eat(MEAL);
                }
                let fridge = self.target.unwrap_or(ctx.body.center_position());
                ctx.think.log.push(format!(
                    "{} ate at the fridge at {}, {}",
                    ctx.body.uid(),
                    fridge.x,
                    fridge.y
                ));
                self.forget();
                return GoalProgress::Achieved;
            }
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again.
        if Self::carrying_food(ctx) {
            // Food already in hand — from a meal that was interrupted, say —
            // needs no fridge.
            self.target = self.target.or(Some(ctx.body.center_position()));
            let _ = self.queue_from(ctx, Stage::Eating);
            return GoalProgress::Working;
        }
        let here = ctx.body.center_position();
        let Some(fridge) = ctx.think.features.nearest(FeatureKind::Food, here) else {
            return GoalProgress::Blocked;
        };
        self.target = Some(fridge);
        if self.queue_from(ctx, Stage::Walking) {
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
