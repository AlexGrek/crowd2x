//! [`Stow`]: put what is in the hand away in the pocket, to be kept.

use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::item::ItemKind;

use super::super::action::{Action, ActionState};
use super::super::task::{self, SavedTask, Task, TaskCtx, TaskExecutor, TaskResult};

/// Half a world minute to put something away.
pub const STOW_SECONDS: f32 = watched(0.5 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Stow {
    pub seconds: f32,
}

impl Stow {
    /// What is in the hand, if the pocket has room for it **once the hand is
    /// empty**: the inventory counts what is in the hand against the load it
    /// can take, so asking it with the item still held counts it twice.
    fn stowable(ctx: &TaskCtx<'_>) -> Option<ItemKind> {
        let inventory = ctx.inventory.as_deref()?;
        let item = inventory.hand()?;
        let mut probe = *inventory;
        let _ = probe.set_hand(None);
        probe.room_for(item).then_some(item)
    }
}

impl TaskExecutor for Stow {
    /// Fails with nothing in the hand, or no room for it — checked every
    /// tick. **It is in the pocket only once the action is over.**
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let Some(item) = Stow::stowable(ctx) else {
            return TaskResult::Failed;
        };
        if ctx.action.is_none() {
            *ctx.action = Action::wait(self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => match ctx.inventory.as_deref_mut() {
                Some(inventory) => {
                    let _ = inventory.set_hand(None);
                    if inventory.stow(item) {
                        TaskResult::Success
                    } else {
                        let _ = inventory.set_hand(Some(item));
                        TaskResult::Failed
                    }
                }
                None => TaskResult::Failed,
            },
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        "put it in the pocket".to_string()
    }
}

impl Task {
    /// Put what is in the hand away in the pocket.
    pub const fn stow(seconds: f32) -> Task {
        Task::Stow(Stow { seconds })
    }
}

/// Saved as `stow`, with no cell.
impl SavedTask for Stow {
    fn save(&self) -> pb::TaskKind {
        task::named("stow", None, self.seconds)
    }

    fn restore(saved: &pb::TaskKind) -> Option<Result<Stow, Invalid>> {
        let (_, seconds) = task::read_named(saved, "stow")?;
        Some(task::seconds(seconds).map(|seconds| Stow { seconds }))
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
    fn it_puts_the_hand_away_in_the_pocket_when_done() {
        let mut rig = rig();
        rig.set_hand(Some(ItemKind::Food));
        let mut task = Task::stow(0.3);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!((rig.hand(), rig.inventory.count(ItemKind::Food)), (Some(ItemKind::Food), 0), "not until it is away");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!((rig.hand(), rig.inventory.count(ItemKind::Food)), (None, 1));
    }

    /// A pocket packed to its very limit still takes what is in the hand,
    /// which the inventory had already counted: three pieces of scrap, a
    /// drink in the hand and a meal stowed come to exactly ten kilos.
    #[test]
    fn it_fits_what_is_in_the_hand_to_the_last_gram_it_is_counted_once() {
        let mut rig = rig();
        for _ in 0..3 {
            assert!(rig.inventory.stow(ItemKind::Scrap));
        }
        assert!(rig.inventory.stow(ItemKind::Food));
        rig.set_hand(Some(ItemKind::Water));
        assert_eq!(rig.inventory.mass(), 10.0);
        assert_eq!(rig.run(&mut Task::stow(0.1), 600), TaskResult::Success);
        assert_eq!((rig.hand(), rig.inventory.count(ItemKind::Water)), (None, 1));
        assert_eq!(rig.inventory.mass(), 10.0);
    }

    #[test]
    fn it_fails_with_nothing_in_the_hand_or_no_room_and_keeps_the_hand() {
        let mut rig = rig();
        assert_eq!(rig.tick(&mut Task::stow(0.1)), TaskResult::Failed);
        for _ in 0..3 {
            assert!(rig.inventory.stow(ItemKind::Scrap));
        }
        rig.set_hand(Some(ItemKind::Steel));
        assert_eq!(rig.tick(&mut Task::stow(0.1)), TaskResult::Failed, "nine kilos and five more");
        assert_eq!(rig.hand(), Some(ItemKind::Steel));
    }

    #[test]
    fn it_is_saved_and_a_stranger_refused() {
        let task = Task::stow(2.0);
        assert_eq!(Task::restore(&task.save()).unwrap(), task);
        assert!(Task::restore(&pb::Task { kind: Some(task::named("stow", None, f32::NAN)) }).is_err());
    }
}
