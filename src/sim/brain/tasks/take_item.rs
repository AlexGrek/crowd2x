//! [`TakeItem`]: take something out of whatever is in a cell, from beside it.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::item::ItemKind;

use super::super::action::{Action, ActionState};
use super::super::task::{self, SavedTask, Task, TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TakeItem {
    /// Where it is taken from — the fridge's cell.
    pub from: Point,
    pub item: ItemKind,
    /// How long taking it takes.
    pub seconds: f32,
}

impl TaskExecutor for TakeItem {
    /// **Only from one step away, checked every tick.** This task does not
    /// walk: the walking is a task that should have been queued in front of
    /// it, and an interaction that quietly walked would hide a goal that forgot
    /// to plan the route. A unit with no hands, or its hand already full,
    /// cannot take anything either.
    ///
    /// The item is in hand only once the action is over — a unit interrupted
    /// halfway through getting food out of the fridge has none.
    ///
    /// **When `from` is a fridge, its door has to be open for the action to
    /// *start* — and that is checked here only, not every tick while it
    /// runs.** Every other precondition in this file is checked on every
    /// tick (rule 7 in the brain-engineer skill); this is a deliberate
    /// exception. Re-checking the door while the action is under way would
    /// fail every other unit mid-take the moment anybody closed it, and with
    /// several units at one fridge that is a livelock rather than a bug: food
    /// already coming out is not put back because somebody shut the door.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.from) > 1 {
            return TaskResult::Failed;
        }
        let Some(inventory) = ctx.inventory.as_deref() else {
            return TaskResult::Failed;
        };
        // These must remain true throughout the take, including the tick
        // that commits the item. A command can fill the hand mid-action.
        if inventory.hand().is_some() {
            return TaskResult::Failed;
        }
        let fridge = ctx.think.fridges.get(self.from);
        if fridge.is_some_and(|state| !state.is_powered()) {
            return TaskResult::Failed;
        }
        // Nothing to eat in a fridge while the store's superfridge is empty
        // (`sim::superfridge`): water only.
        if fridge.is_some() && matches!(self.item, ItemKind::Food | ItemKind::DogFood) && !ctx.think.food_in_fridges() {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            // Only the door is a start condition: another visitor closing
            // it must not continually interrupt everybody else's take.
            if fridge.is_some_and(|state| !state.is_open()) {
                return TaskResult::Failed;
            }
            *ctx.action = Action::interact(self.from, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(inventory) = ctx.inventory.as_deref_mut() {
                    // Into the hand, which is never refused however laden the
                    // unit is — see `Inventory`. Nothing was in it: that was
                    // checked before the action started and every tick since.
                    let _ = inventory.set_hand(Some(self.item));
                }
                if fridge.is_some() {
                    *ctx.effect = crate::sim::Effect::Buy { item: self.item };
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("take {} from {}, {}", self.item.name(), self.from.x, self.from.y)
    }
}

impl Task {
    /// Take `item` out of whatever is in `from`, from a cell beside it.
    pub const fn take(from: Point, item: ItemKind, seconds: f32) -> Task {
        Task::TakeItem(TakeItem { from, item, seconds })
    }
}

/// Saved as a take of its own (`pb::TakeItem`): from where, what — by the
/// item's name — and how long.
impl SavedTask for TakeItem {
    fn save(&self) -> pb::TaskKind {
        pb::TaskKind::TakeItem(pb::TakeItem { from: Some(self.from.into()), item: self.item.name().to_string(), seconds: self.seconds })
    }

    fn restore(saved: &pb::TaskKind) -> Option<Result<TakeItem, Invalid>> {
        let pb::TaskKind::TakeItem(task) = saved else {
            return None;
        };
        Some((|| Ok(TakeItem { from: pb::cell(task.from, "take")?, item: task::item(&task.item)?, seconds: task::seconds(task.seconds)? }))())
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::super::super::task::Task;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::testing::prop_at;

    fn rig_at(cell: Point) -> Rig {
        Rig::new(Map::new(Size::new(9, 9), FLOOR), cell)
    }

    fn rig_beside_a_fridge(fridge: Point) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        prop_at(&mut map, "fridge", fridge);
        Rig::new(map, Point::new(fridge.x - 1, fridge.y))
    }

    #[test]
    fn taking_from_more_than_a_step_away_fails_without_starting() {
        let mut rig = rig_at(Point::new(1, 1));
        let mut task = Task::take(Point::new(5, 5), ItemKind::Food, 1.0);

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.hand(), None);
    }

    #[test]
    fn taking_puts_the_item_in_hand_only_once_the_action_is_done() {
        let mut rig = rig_at(Point::new(4, 5));
        let mut task = Task::take(Point::new(5, 5), ItemKind::Food, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.hand(), None, "not yet: the fridge is still being opened");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.hand(), Some(ItemKind::Food));
        assert_eq!(rig.effect, crate::sim::Effect::None, "a non-fridge does not charge");
    }

    #[test]
    fn taking_from_a_fridge_charges_only_when_the_item_reaches_the_hand() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_beside_a_fridge(fridge);
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::take(fridge, ItemKind::Food, 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, crate::sim::Effect::None);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert_eq!(rig.effect, crate::sim::Effect::Buy { item: ItemKind::Food });
    }

    #[test]
    fn an_empty_central_reserve_refuses_food_and_dog_food_but_keeps_water_available() {
        for item in [ItemKind::Food, ItemKind::DogFood, ItemKind::Water] {
            let fridge = Point::new(5, 5);
            let reserve = Point::new(2, 2);
            let mut map = Map::new(Size::new(9, 9), FLOOR);
            prop_at(&mut map, "fridge", fridge);
            prop_at(&mut map, "superfridge", reserve);
            assert!(map.set_initial_stock(reserve, 0));
            // Authored zero must also survive the map's disk format.
            let map = Map::from_json(&map.to_json().unwrap()).unwrap();
            let mut rig = Rig::new(map, Point::new(4, 5));
            assert!(rig.world.fridges.set_open(fridge, true));
            let result = rig.run(&mut Task::take(fridge, item, 0.2), 100);
            if item == ItemKind::Water {
                assert_eq!(result, TaskResult::Success);
                assert_eq!(rig.hand(), Some(ItemKind::Water));
            } else {
                assert_eq!(result, TaskResult::Failed);
                assert_eq!(rig.hand(), None);
                assert_eq!(rig.effect, crate::sim::Effect::None);
            }
        }
    }

    #[test]
    fn central_stock_disappearing_during_a_take_leaves_the_hand_empty() {
        let fridge = Point::new(5, 5);
        let reserve = Point::new(2, 2);
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        prop_at(&mut map, "fridge", fridge);
        prop_at(&mut map, "superfridge", reserve);
        let mut rig = Rig::new(map, Point::new(4, 5));
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::take(fridge, ItemKind::Food, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert!(rig.world.bins.set(reserve, 0));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.hand(), None);
        assert_eq!(rig.effect, crate::sim::Effect::None);
    }

    /// Its box switched off on the way there: open or not, a fridge with no
    /// power is not somewhere food comes out of.
    #[test]
    fn taking_from_a_fridge_with_no_power_fails_without_starting() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_beside_a_fridge(fridge);
        let dead = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.fridges = crate::sim::fridge::Fridges::from_map(&rig.world.map, &dead);
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::take(fridge, ItemKind::Food, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.hand(), None);
    }

    #[test]
    fn taking_with_full_hands_fails() {
        let mut rig = rig_at(Point::new(4, 5));
        rig.set_hand(Some(ItemKind::Food));
        let mut task = Task::take(Point::new(5, 5), ItemKind::Food, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
    }

    #[test]
    fn filling_the_hand_mid_take_fails_without_overwriting_what_arrived() {
        for remaining in [0.5, 0.01] {
            let mut rig = rig_at(Point::new(4, 5));
            let mut task = Task::take(Point::new(5, 5), ItemKind::Food, remaining);
            assert_eq!(rig.tick(&mut task), TaskResult::Executing);
            rig.set_hand(Some(ItemKind::Water));
            // The short duration would finish on this very tick.
            assert_eq!(rig.tick(&mut task), TaskResult::Failed);
            assert_eq!(rig.hand(), Some(ItemKind::Water));
        }
    }

    #[test]
    fn losing_power_mid_take_fails_without_delivering_an_item() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_beside_a_fridge(fridge);
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::take(fridge, ItemKind::Food, 0.01);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let dead = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.fridges.set_power(&dead);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.hand(), None);
    }

    #[test]
    fn walking_away_mid_take_fails_it() {
        let mut rig = rig_at(Point::new(4, 5));
        let mut task = Task::take(Point::new(5, 5), ItemKind::Food, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);

        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.hand(), None);
    }

    #[test]
    fn taking_from_a_closed_fridge_fails_without_starting() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_beside_a_fridge(fridge);
        let mut task = Task::take(fridge, ItemKind::Food, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.hand(), None);
    }

    #[test]
    fn taking_from_an_open_fridge_works() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_beside_a_fridge(fridge);
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::take(fridge, ItemKind::Food, 0.5);

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.hand(), Some(ItemKind::Food));
    }

    #[test]
    fn once_taking_has_started_the_door_closing_does_not_fail_it() {
        // A deliberate exception to checking a precondition every tick: the
        // door only has to be open for the take to *start*. Otherwise
        // several units at one fridge would livelock each other's meals
        // every time somebody shut it.
        let fridge = Point::new(5, 5);
        let mut rig = rig_beside_a_fridge(fridge);
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::take(fridge, ItemKind::Food, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert!(rig.world.fridges.set_open(fridge, false), "somebody else closed it mid-take");

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.hand(), Some(ItemKind::Food));
    }

    #[test]
    fn taking_from_a_cell_with_no_fridge_in_it_is_unchanged() {
        // No fridge at all at `from`: the door check has nothing to ask about
        // and does not get in the way.
        let mut rig = rig_at(Point::new(4, 5));
        let mut task = Task::take(Point::new(5, 5), ItemKind::Food, 0.5);

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.hand(), Some(ItemKind::Food));
    }
}
