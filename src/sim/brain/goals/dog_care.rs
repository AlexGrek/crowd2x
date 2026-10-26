//! [`DogCareGoal`]: an owner feeding its dog.
//!
//! A dog cannot feed itself. When its owner sees it hungry
//! ([`KeepRoutine`](super::super::routines::KeepRoutine)) this goal does what
//! eating does for a person, with a different dinner: **walk beside the
//! nearest fridge it may use, open it, take dog food out of it** — which costs
//! what dog food costs (`economy.yaml`, a dollar), like any other thing taken
//! from a fridge — **close it, and give it to the dog**.
//!
//! The dog is not a place: it moves, so it is not planned for before the food
//! is in hand. With the food, the goal asks where the dog is now (the
//! owner's [`Bond`](crate::sim::pets::Bond)), walks beside it if it is not
//! within [`FEED_REACH`](crate::sim::pets::FEED_REACH) and feeds it
//! ([`Task::feed`]); a dog that wandered off in the meantime, or while it was
//! being fed, is gone after again, up to [`MAX_CHASES`] times a visit.
//! Hungry dogs keep close to their owners ([`FollowGoal`](super::FollowGoal)),
//! so it is seldom more than once.

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::inventory::Inventory;
use crate::sim::item::ItemKind;
use crate::sim::pets::FEED_REACH;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::{CLOSE_SECONDS, FEED_SECONDS, OPEN_SECONDS};
use super::eat::TAKE_SECONDS;
use super::{choose_nearest, nearest_in_reach, stand_beside, Seek, Stand};

/// How many times one visit goes after a dog that was not where it was
/// before it gives up.
pub const MAX_CHASES: u8 = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DogCareGoal {
    /// The fridge it is fetching dog food from, and how the way there has
    /// gone.
    seek: Seek,
    /// Times it has gone after the dog and not been in reach of it.
    chases: u8,
}

impl DogCareGoal {
    pub fn new() -> DogCareGoal {
        DogCareGoal::default()
    }

    fn forget(&mut self) {
        *self = DogCareGoal::default();
    }

    /// Queue fetching dog food **from what is true now**: walk beside the
    /// fridge unless already there, open it, take dog food, close it. Anything
    /// else in the hand is finished first, as eating does, since taking needs
    /// an empty hand. `false` when there is nowhere to stand to use it.
    fn fetch(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(fridge) = self.seek.target else {
            return false;
        };
        let stand = stand_beside(ctx, fridge);
        if let Stand::Nowhere = stand {
            return false;
        }
        if let Some(other) = held(ctx) {
            let _ = ctx.tasks.push_back(Task::consume(other, other.consume_seconds()));
        }
        if let Stand::At(cell) = stand {
            let _ = ctx.tasks.push_back(Task::move_beside(cell));
        }
        let _ = ctx.tasks.push_back(Task::open_fridge(fridge, OPEN_SECONDS));
        let _ = ctx.tasks.push_back(Task::take(fridge, ItemKind::DogFood, TAKE_SECONDS));
        let _ = ctx.tasks.push_back(Task::close_fridge(fridge, CLOSE_SECONDS));
        true
    }

    /// The dog food is in hand: feed the dog if it is in reach, else go to it.
    /// `false` when there is nowhere to stand beside it.
    fn serve(&mut self, ctx: &mut GoalCtx<'_>, dog: Point) -> bool {
        // The fridge it came from, left open if it was called away from it.
        if let Some(fridge) = self.seek.target
            && ctx.body.center_position().manhattan_distance(fridge) <= 1
            && ctx.think.fridges.is_open(fridge)
        {
            let _ = ctx.tasks.push_back(Task::close_fridge(fridge, CLOSE_SECONDS));
        }
        if ctx.body.center_position().chebyshev_distance(dog) <= FEED_REACH {
            let _ = ctx.tasks.push_back(Task::feed(dog, FEED_SECONDS));
            return true;
        }
        if let Stand::Nowhere = stand_beside(ctx, dog) {
            return false;
        }
        let _ = ctx.tasks.push_back(Task::move_beside(dog));
        true
    }

    /// The dog was not in reach: go after it again, or give up the visit.
    fn chase(&mut self, ctx: &mut GoalCtx<'_>) -> GoalProgress {
        ctx.tasks.clear();
        self.chases += 1;
        if self.chases > MAX_CHASES {
            self.forget();
            return GoalProgress::Blocked;
        }
        GoalProgress::Working
    }
}

fn held(ctx: &GoalCtx<'_>) -> Option<ItemKind> {
    ctx.inventory.and_then(Inventory::hand)
}

impl GoalExecutor for DogCareGoal {
    fn goal(&self) -> GoalId {
        GoalId::DogCare
    }

    fn save(&self) -> pb::Executor {
        let DogCareGoal { seek, chases } = self;
        pb::Executor::DogCare(pb::DogCare { seek: Some(seek.save()), chases: *chases as u32 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::DogCare(care) = saved else {
            return invalid("dog care is a seek and a count");
        };
        let Ok(chases) = u8::try_from(care.chases) else {
            return invalid(format!("dog care: {} chases", care.chases));
        };
        self.seek = Seek::restore(pb::required(&care.seek, "dog care seek")?)?;
        self.chases = chases;
        Ok(())
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() || ctx.inventory.is_none() {
            return GoalProgress::Blocked;
        }
        // A body in hand is seen to the crematorium first, as for a meal.
        if held(ctx) == Some(ItemKind::Body) {
            return GoalProgress::Blocked;
        }
        let Some(dog) = ctx.think.bonds.partner(ctx.body.uid()).map(|bond| bond.cell) else {
            // No dog in the world to feed.
            self.forget();
            return GoalProgress::Blocked;
        };
        let holding = held(ctx) == Some(ItemKind::DogFood);

        match (last, ctx.finished) {
            (TaskResult::Success, Some(Task::Feed(_))) => {
                self.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Failed, Some(Task::Feed(_))) => return self.chase(ctx),
            (TaskResult::Failed, Some(Task::MoveTo(_))) if holding => return self.chase(ctx),
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                if self.seek.setback(ctx, |ctx| nearest_in_reach(ctx, FeatureKind::Food)) && self.fetch(ctx) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Taking went wrong — a hand already full, a fridge off. Cheap
                // to work out again from scratch.
                ctx.tasks.clear();
                self.seek.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again from what is true now.
        if holding {
            return if self.serve(ctx, dog) {
                GoalProgress::Working
            } else {
                self.forget();
                GoalProgress::Blocked
            };
        }
        let Some(fridge) = choose_nearest(ctx, FeatureKind::Food) else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(fridge);
        if self.fetch(ctx) {
            GoalProgress::Working
        } else {
            self.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "fridge",
                self.seek.target.map_or_else(|| "none".to_string(), |cell| format!("{}, {}", cell.x, cell.y)),
            ),
            ("chases", self.chases.to_string()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::economy::Money;
    use crate::sim::pets::rig::{dog, dog_uid, owner_uid};
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::GameEntity;

    const FRIDGE: Point = Point::new(15, 5);

    /// A hall with a fridge at the end, a person with every need met and ten
    /// dollars who keeps a dog `hunger` hungry beside it.
    fn kennel(hunger: f32) -> (World, crate::sim::kinds::Human, crate::sim::kinds::Dog) {
        let mut map = Map::new(Size::new(20, 10), FLOOR);
        prop_at(&mut map, "fridge", FRIDGE);
        let mut world = World::new(map);
        let mut owner = needy_human(Point::new(5, 5), 0.0, 0.0, 0.0);
        assert!(owner.set_companion(dog_uid()));
        world.economy.open(owner_uid(), Money::dollars(10));
        (world, owner, dog(Point::new(6, 5), hunger))
    }

    /// The owner first, so its bond is there for the dog to follow.
    fn tick(world: &mut World, owner: &mut crate::sim::kinds::Human, dog: &mut crate::sim::kinds::Dog) {
        world.step(owner);
        world.step(dog);
    }

    #[test]
    fn an_owner_buys_dog_food_for_a_dollar_and_feeds_its_hungry_dog() {
        let (mut world, mut owner, mut dog) = kennel(80.0);
        for _ in 0..4000 {
            tick(&mut world, &mut owner, &mut dog);
            if world.log_contains("fed") && dog.gut().hunger() < 50.0 {
                break;
            }
        }
        assert!(world.log_contains(&format!("{} fed {}", owner_uid(), dog_uid())), "{:?}", world.lines());
        assert!(world.log_contains("paid $1.00 for dog food"), "{:?}", world.lines());
        assert_eq!(world.economy.account(owner_uid()).unwrap().cash, Money::dollars(9));
        assert!(dog.gut().hunger() < 50.0, "still {} hungry", dog.gut().hunger());
        assert!(dog.gut().digesting() > 0.0, "the meal is inside it");
        assert_eq!(owner.carried(), None, "the dog food was given, not kept");
    }

    /// The meal is felt by the dog the tick after it is served, but its bond
    /// counts it from the moment it is served: on the very next tick the owner
    /// does not take the dog for hungry and set off for another dinner.
    #[test]
    fn one_dinner_is_bought_and_no_second_one_while_the_dog_is_fed() {
        let (mut world, mut owner, mut dog) = kennel(80.0);
        let served = format!("{} fed {}", owner_uid(), dog_uid());
        for _ in 0..4000 {
            world.step(&mut owner);
            if world.log_contains(&served) {
                break;
            }
            world.step(&mut dog);
        }
        assert!(world.log_contains(&served), "never fed");
        world.step(&mut dog);
        world.step(&mut owner);
        assert_ne!(owner.brain().top_goal(), GoalId::DogCare, "went for a second dinner");
        assert!(dog.gut().hunger() < 50.0);
        for _ in 0..3000 {
            tick(&mut world, &mut owner, &mut dog);
        }
        assert_eq!(world.lines().iter().filter(|line| line.contains("for dog food")).count(), 1, "{:?}", world.lines());
        assert_eq!(world.economy.account(owner_uid()).unwrap().cash, Money::dollars(9));
    }

    #[test]
    fn a_dog_that_is_not_hungry_is_not_fed() {
        let (mut world, mut owner, mut dog) = kennel(20.0);
        for _ in 0..3000 {
            tick(&mut world, &mut owner, &mut dog);
        }
        assert!(!world.log_contains("dog food"));
        assert_eq!(world.economy.account(owner_uid()).unwrap().cash, Money::dollars(10));
        assert_ne!(owner.brain().top_goal(), GoalId::DogCare);
    }

    /// Nowhere to get dog food: held off, and no money spent or crash.
    #[test]
    fn an_owner_with_no_fridge_is_held_off() {
        let mut world = World::new(Map::new(Size::new(20, 10), FLOOR));
        let mut owner = needy_human(Point::new(5, 5), 0.0, 0.0, 0.0);
        assert!(owner.set_companion(dog_uid()));
        let mut dog = dog(Point::new(6, 5), 90.0);
        let mut held_off = false;
        for _ in 0..1500 {
            tick(&mut world, &mut owner, &mut dog);
            held_off |= owner.brain().goals().cooldown(GoalId::DogCare) > 0;
        }
        assert!(held_off);
        assert_eq!(dog.gut().hunger() >= 90.0, true, "nobody fed it");
    }

    #[test]
    fn the_fridge_and_the_chases_are_saved_and_a_broken_save_refused() {
        let goal = DogCareGoal { seek: Seek::at(FRIDGE), chases: 3 };
        let mut back = DogCareGoal::new();
        back.restore(&goal.save()).unwrap();
        assert_eq!(back, goal);
        assert!(back.restore(&pb::Executor::DogCare(pb::DogCare { seek: Some(goal.seek.save()), chases: 256 })).is_err());
        assert!(back.restore(&pb::Executor::DogCare(pb::DogCare { seek: None, chases: 0 })).is_err());
        assert!(back.restore(&pb::Executor::Seek(pb::Seek::default())).is_err());
    }
}
