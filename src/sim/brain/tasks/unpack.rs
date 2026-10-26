//! [`Unpack`]: take one thing out of the pocket and into the hand — a meal,
//! to be eaten out in the wasteland where there is no fridge to fetch it from.

use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::item::ItemKind;

use super::super::action::{Action, ActionState};
use super::super::task::{self, SavedTask, Task, TaskCtx, TaskExecutor, TaskResult};

/// Half a world minute of feeling about in a pocket.
pub const UNPACK_SECONDS: f32 = watched(0.5 * MINUTE);

/// The name a save keeps it under, with the item's after it.
const NAME: &str = "unpack ";

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Unpack {
    pub item: ItemKind,
    pub seconds: f32,
}

impl TaskExecutor for Unpack {
    /// Fails with a hand already full or none of it stowed — checked every
    /// tick, so an item somehow gone mid-search ends the search. **It is in
    /// the hand only once the action is over**: a unit cut short has its
    /// pocket as it was.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let Some(inventory) = ctx.inventory.as_deref() else {
            return TaskResult::Failed;
        };
        if inventory.hand().is_some() || inventory.count(self.item) == 0 {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::wait(self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => match ctx.inventory.as_deref_mut() {
                Some(inventory) => {
                    if inventory.take_out(self.item) {
                        let _ = inventory.set_hand(Some(self.item));
                        TaskResult::Success
                    } else {
                        TaskResult::Failed
                    }
                }
                None => TaskResult::Failed,
            },
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("take {} out of the pocket", self.item.name())
    }
}

impl Task {
    /// Take one `item` out of the pocket, into the hand.
    pub const fn unpack(item: ItemKind, seconds: f32) -> Task {
        Task::Unpack(Unpack { item, seconds })
    }
}

/// Saved by name, `unpack food` and the like, with no cell: no schema of its
/// own, as a task that is a place and a time has none.
impl SavedTask for Unpack {
    fn save(&self) -> pb::TaskKind {
        task::named(&format!("{NAME}{}", self.item.name()), None, self.seconds)
    }

    fn restore(saved: &pb::TaskKind) -> Option<Result<Unpack, Invalid>> {
        let pb::TaskKind::Named(named) = saved else {
            return None;
        };
        let item = named.name.strip_prefix(NAME)?;
        Some((|| Ok(Unpack { item: task::item(item)?, seconds: task::seconds(named.seconds)? }))())
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Point, Size, FLOOR};

    fn rig() -> Rig {
        Rig::new(Map::new(Size::new(5, 5), FLOOR), Point::new(2, 2))
    }

    #[test]
    fn it_takes_the_meal_out_of_the_pocket_and_into_the_hand_when_done() {
        let mut rig = rig();
        assert!(rig.inventory.stow(ItemKind::Food));
        let mut task = Task::unpack(ItemKind::Food, 0.3);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!((rig.hand(), rig.inventory.count(ItemKind::Food)), (None, 1), "not until it is found");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!((rig.hand(), rig.inventory.count(ItemKind::Food)), (Some(ItemKind::Food), 0));
    }

    #[test]
    fn it_takes_only_what_is_asked_for_and_leaves_the_rest() {
        let mut rig = rig();
        assert!(rig.inventory.stow(ItemKind::Food) && rig.inventory.stow(ItemKind::Water));
        assert_eq!(rig.run(&mut Task::unpack(ItemKind::Water, 0.1), 600), TaskResult::Success);
        assert_eq!(rig.hand(), Some(ItemKind::Water));
        assert_eq!(rig.inventory.count(ItemKind::Food), 1);
    }

    #[test]
    fn it_fails_with_nothing_to_find_or_a_hand_already_full() {
        let mut rig = rig();
        assert_eq!(rig.tick(&mut Task::unpack(ItemKind::Food, 0.1)), TaskResult::Failed);
        assert!(rig.inventory.stow(ItemKind::Food));
        rig.set_hand(Some(ItemKind::Water));
        assert_eq!(rig.tick(&mut Task::unpack(ItemKind::Food, 0.1)), TaskResult::Failed);
        assert_eq!(rig.inventory.count(ItemKind::Food), 1, "nothing lost");
    }

    #[test]
    fn it_is_saved_by_the_item_it_is_for_and_a_stranger_refused() {
        let task = Task::unpack(ItemKind::Water, 2.0);
        assert_eq!(Task::restore(&task.save()).unwrap(), task);
        assert_ne!(task.save(), Task::unpack(ItemKind::Food, 2.0).save());
        let stranger = pb::Task { kind: Some(task::named("unpack nothing", None, 1.0)) };
        assert!(Task::restore(&stranger).is_err());
    }
}
