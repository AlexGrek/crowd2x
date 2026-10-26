//! An owner giving its dog the dog food in its hand. The dog is the one its
//! own bond names ([`Bonds::partner`](crate::sim::pets::Bonds::partner)),
//! found again every tick wherever it has wandered to; the world serves the
//! meal ([`Effect::Feed`]) and the dog feels it in its own react.

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::item::ItemKind;
use crate::sim::pets::FEED_REACH;
use crate::sim::Effect;

/// A minute of world to put a bowl down and see the dog to it.
pub const FEED_SECONDS: f32 = watched(MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Feed {
    /// Where the dog stood when the feeding was planned: where the walk went,
    /// and what a debugger is told. Who is fed is the owner's bond's.
    pub at: Point,
    pub seconds: f32,
}

impl TaskExecutor for Feed {
    /// Dog food in the hand, and the dog within [`FEED_REACH`] — checked every
    /// tick, so a dog that wanders off mid-feed is not fed and the goal goes
    /// after it again. The food is gone from the hand only once it is given.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.inventory.as_deref().and_then(|inventory| inventory.hand()) != Some(ItemKind::DogFood) {
            return TaskResult::Failed;
        }
        let Some(dog) = ctx.think.bonds.partner(ctx.walk.body().uid()) else {
            return TaskResult::Failed;
        };
        if ctx.here().chebyshev_distance(dog.cell) > FEED_REACH {
            return TaskResult::Failed;
        }
        let (dog, cell) = (dog.who, dog.cell);
        if ctx.action.is_none() {
            *ctx.action = Action::interact(cell, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(inventory) = ctx.inventory.as_deref_mut() {
                    let _ = inventory.set_hand(None);
                }
                *ctx.effect = Effect::Feed { dog };
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("feed the dog at {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    pub const fn feed(at: Point, seconds: f32) -> Task {
        Task::Feed(Feed { at, seconds })
    }
}

impl TimedTask for Feed {
    const NAME: &'static str = "feed";
    fn new(at: Point, seconds: f32) -> Self {
        Self { at, seconds }
    }
    fn cell(&self) -> Point {
        self.at
    }
    fn seconds(&self) -> f32 {
        self.seconds
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::pets::Bond;
    use crate::sim::uid::{EntityType, Uid};

    fn owner() -> Uid {
        Uid::new(EntityType::Human, 9)
    }

    fn dog() -> Uid {
        Uid::new(EntityType::Dog, 77)
    }

    /// A rig holding dog food, bonded to a dog standing at `dog_at`.
    fn rig(dog_at: Point) -> Rig {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), Point::new(4, 4));
        rig.set_hand(Some(ItemKind::DogFood));
        let bonds = &mut rig.world.facilities.bonds;
        bonds.set(owner(), Some(Bond { who: owner(), with: dog(), cell: Point::new(4, 4), hunger: 0.0, bowel: 0.0, asleep: false }));
        bonds.set(dog(), Some(Bond { who: dog(), with: owner(), cell: dog_at, hunger: 70.0, bowel: 0.0, asleep: false }));
        rig
    }

    #[test]
    fn it_gives_the_dog_food_to_the_dog_and_the_hand_is_empty() {
        let mut rig = rig(Point::new(5, 5));
        let mut task = Task::feed(Point::new(5, 5), 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.hand(), Some(ItemKind::DogFood), "not until it is eaten");
        assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Feed { dog: dog() });
        assert_eq!(rig.hand(), None);
    }

    #[test]
    fn it_fails_with_the_dog_out_of_reach_even_halfway_through() {
        let mut rig = rig(Point::new(5, 5));
        let mut task = Task::feed(Point::new(5, 5), 5.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.world.facilities.bonds.set(
            dog(),
            Some(Bond { who: dog(), with: owner(), cell: Point::new(8, 8), hunger: 70.0, bowel: 0.0, asleep: false }),
        );
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.hand(), Some(ItemKind::DogFood), "kept for the next try");
    }

    #[test]
    fn it_fails_with_nothing_to_give_or_nobody_to_give_it_to() {
        let mut rig = rig(Point::new(5, 5));
        rig.set_hand(None);
        assert_eq!(rig.tick(&mut Task::feed(Point::new(5, 5), 0.2)), TaskResult::Failed);
        rig.set_hand(Some(ItemKind::Food));
        assert_eq!(rig.tick(&mut Task::feed(Point::new(5, 5), 0.2)), TaskResult::Failed, "food is not dog food");
        rig.set_hand(Some(ItemKind::DogFood));
        rig.world.facilities.bonds.set(dog(), None);
        assert_eq!(rig.tick(&mut Task::feed(Point::new(5, 5), 0.2)), TaskResult::Failed);
    }
}
