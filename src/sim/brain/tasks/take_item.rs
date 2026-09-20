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
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.from) > 1 {
            return TaskResult::Failed;
        }
        let Some(inventory) = ctx.inventory.as_deref() else {
            return TaskResult::Failed;
        };
        if ctx.action.is_none() {
            if inventory.hand().is_some() {
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

    fn rig_at(cell: Point) -> Rig {
        Rig::new(Map::new(Size::new(9, 9), FLOOR), cell)
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

    #[test]
    fn taking_with_full_hands_fails() {
        let mut rig = rig_at(Point::new(4, 5));
        rig.set_hand(Some(ItemKind::Food));
        let mut task = Task::take(Point::new(5, 5), ItemKind::Food, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
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
}
