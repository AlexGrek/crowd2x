//! [`TakeItem`]: take something out of whatever is in a cell, from beside it.

use crate::map::Point;
use crate::sim::item::ItemKind;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

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
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("take {} from {}, {}", self.item.name(), self.from.x, self.from.y)
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
