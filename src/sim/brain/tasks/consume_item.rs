//! [`ConsumeItem`]: use up what is in hand.

use crate::sim::biology::Event;
use crate::sim::item::ItemKind;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ConsumeItem {
    pub item: ItemKind,
    /// How long using it up takes.
    pub seconds: f32,
}

impl ConsumeItem {
    fn holding_it(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.carried.as_deref() == Some(&Some(self.item))
    }
}

impl TaskExecutor for ConsumeItem {
    /// Fails without the item in hand — checked every tick, so food that is
    /// somehow gone mid-meal ends the meal — or without a body to consume it.
    /// Finishing empties the hand and tells the body it was ingested; what
    /// that does is the body's processes' to say.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.holding_it(ctx) || ctx.biology.is_none() {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::consume(self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(hand) = ctx.carried.as_deref_mut() {
                    *hand = None;
                }
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Ingested(self.item));
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("consume {}", self.item.name())
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::super::super::task::Task;
    use super::*;
    use crate::map::{Map, Point, Size, FLOOR};
    use crate::sim::item::{DRINK, MEAL};

    fn rig() -> Rig {
        Rig::new(Map::new(Size::new(5, 5), FLOOR), Point::new(2, 2))
    }

    #[test]
    fn consuming_food_empties_the_hand_and_takes_hunger_away() {
        let mut rig = rig();
        rig.carried = Some(ItemKind::Food);
        rig.biology.edit(|stats| stats.with_hunger(90.0));
        let mut task = Task::consume(ItemKind::Food, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.biology.stats().hunger(), 90.0, "nothing eaten until the meal is over");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.carried, None);
        assert_eq!(rig.biology.stats().hunger(), 90.0 - MEAL);
    }

    #[test]
    fn consuming_water_takes_thirst_away_and_leaves_hunger_alone() {
        let mut rig = rig();
        rig.carried = Some(ItemKind::Water);
        rig.biology.edit(|stats| stats.with_hunger(90.0).with_thirst(90.0));
        let mut task = Task::consume(ItemKind::Water, 0.5);

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.carried, None);
        assert_eq!(rig.biology.stats().thirst(), 90.0 - DRINK);
        assert_eq!(rig.biology.stats().hunger(), 90.0);
        assert!(rig.biology.debug_fields().iter().any(|(n, v)| *n == "bladder on its way" && v != "0.0"));
    }

    #[test]
    fn consuming_something_other_than_what_is_in_hand_fails() {
        let mut rig = rig();
        rig.carried = Some(ItemKind::Food);
        let mut task = Task::consume(ItemKind::Water, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.carried, Some(ItemKind::Food));
    }

    #[test]
    fn consuming_with_nothing_in_hand_fails() {
        let mut rig = rig();
        let mut task = Task::consume(ItemKind::Food, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }
}
