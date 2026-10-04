//! [`ConsumeItem`]: use up what is in hand.

use crate::sim::biology::Event;
use crate::sim::inventory::Inventory;
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
        ctx.inventory.as_deref().and_then(Inventory::hand) == Some(self.item)
    }
}

impl TaskExecutor for ConsumeItem {
    /// Fails without the item in hand — checked every tick, so food that is
    /// somehow gone mid-meal ends the meal — or without a body to consume it.
    /// Finishing empties the hand and tells the body it was ingested; what
    /// that does is the body's processes' to say. If the same food was had
    /// recently, the log says it was in recent memory — as filling, less of a
    /// treat.
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
                if let Some(inventory) = ctx.inventory.as_deref_mut() {
                    let _ = inventory.set_hand(None);
                }
                let recalled = ctx.biology.as_deref_mut().and_then(|b| b.handle(Event::Ingested(self.item)));
                super::report_recalled(ctx, recalled);
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
    use crate::sim::item::{DRINK, MEAL, TASTY};

    fn rig() -> Rig {
        Rig::new(Map::new(Size::new(5, 5), FLOOR), Point::new(2, 2))
    }

    #[test]
    fn consuming_food_empties_the_hand_and_takes_hunger_away() {
        let mut rig = rig();
        rig.set_hand(Some(ItemKind::Food));
        rig.biology.edit(|stats| stats.with_hunger(90.0));
        let mut task = Task::consume(ItemKind::Food, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.biology.stats().hunger(), 90.0, "nothing eaten until the meal is over");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.hand(), None);
        assert_eq!(rig.biology.stats().hunger(), 90.0 - MEAL);
    }

    #[test]
    fn consuming_water_takes_thirst_away_and_leaves_hunger_alone() {
        let mut rig = rig();
        rig.set_hand(Some(ItemKind::Water));
        rig.biology.edit(|stats| stats.with_hunger(90.0).with_thirst(90.0));
        let mut task = Task::consume(ItemKind::Water, 0.5);

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.hand(), None);
        assert_eq!(rig.biology.stats().thirst(), 90.0 - DRINK);
        assert_eq!(rig.biology.stats().hunger(), 90.0);
        assert!(rig.biology.debug_fields().iter().any(|(n, v)| *n == "bladder on its way" && v != "0.0"));
    }

    /// The second meal fills as much as the first and is less of a treat, and
    /// the log says why.
    #[test]
    fn a_second_helping_of_the_same_food_is_as_filling_and_less_satisfying() {
        let mut rig = rig();
        rig.biology.edit(|stats| stats.with_hunger(100.0).with_satisfaction(0.0));

        rig.set_hand(Some(ItemKind::Food));
        let mut first = Task::consume(ItemKind::Food, 0.5);
        assert_eq!(rig.run(&mut first, 600), TaskResult::Success);
        let after_first = *rig.biology.stats();
        assert!(!rig.world.log.drain().iter().any(|l| l.contains("in recent memory")), "nothing to remember yet");

        // Starving again, so the stomach has room for a whole second meal.
        rig.biology.edit(|stats| stats.with_hunger(100.0));
        rig.set_hand(Some(ItemKind::Food));
        let mut second = Task::consume(ItemKind::Food, 0.5);
        assert_eq!(rig.run(&mut second, 600), TaskResult::Success);
        let after_second = *rig.biology.stats();

        assert_eq!(100.0 - after_first.hunger(), MEAL, "the first fills a meal's worth");
        assert_eq!(100.0 - after_second.hunger(), MEAL, "and so does the second");
        let treat = (after_first.satisfaction(), after_second.satisfaction() - after_first.satisfaction());
        assert_eq!(treat.0, TASTY, "the first is a whole treat");
        assert!(treat.1 > 0.0 && treat.1 < TASTY * 0.6, "the second is about half of one: {treat:?}");
        let lines = rig.world.log.drain();
        assert!(
            lines.iter().any(|l| l.contains("had food in recent memory")),
            "the log should say so: {lines:?}"
        );
    }

    /// Water has no flavour to tire of, so drinking twice is not a memory.
    #[test]
    fn a_second_glass_of_water_is_not_in_recent_memory() {
        let mut rig = rig();
        for _ in 0..2 {
            rig.set_hand(Some(ItemKind::Water));
            let mut task = Task::consume(ItemKind::Water, 0.5);
            assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        }
        assert!(!rig.world.log.drain().iter().any(|l| l.contains("in recent memory")));
    }

    #[test]
    fn consuming_something_other_than_what_is_in_hand_fails() {
        let mut rig = rig();
        rig.set_hand(Some(ItemKind::Food));
        let mut task = Task::consume(ItemKind::Water, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.hand(), Some(ItemKind::Food));
    }

    #[test]
    fn consuming_with_nothing_in_hand_fails() {
        let mut rig = rig();
        let mut task = Task::consume(ItemKind::Food, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }
}
