//! The task executors that exist: how each small step is carried out.
//!
//! Each is a `Copy` struct holding what the step is about, with an
//! [`TaskExecutor`](super::task::TaskExecutor) impl that starts its action,
//! checks what has to stay true while it runs, and applies what finishing it
//! means. See [`super::task`] for why they are not boxed.

pub mod close_fridge;
pub mod consume_item;
pub mod move_to;
pub mod open_fridge;
pub mod sleep;
pub mod take_item;
pub mod use_computer;
pub mod use_toilet;
pub mod wait;

pub use close_fridge::{CloseFridge, CLOSE_SECONDS};
pub use consume_item::ConsumeItem;
pub use move_to::MoveTo;
pub use open_fridge::{OpenFridge, OPEN_SECONDS};
pub use sleep::Sleep;
pub use take_item::TakeItem;
pub use use_computer::UseComputer;
pub use use_toilet::UseToilet;
pub use wait::Wait;

#[cfg(test)]
pub(crate) mod rig {
    //! One unit's body, hands and biology, with no brain above them, so a task
    //! executor can be run tick by tick on its own.

    use crate::map::{Map, Point};
    use crate::sim::biology::{Biology, Stats};
    use crate::sim::brain::action::Action;
    use crate::sim::brain::task::{Task, TaskCtx, TaskResult};
    use crate::sim::inventory::Inventory;
    use crate::sim::item::ItemKind;
    use crate::sim::testing::World;
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::walker::Walker;
    use crate::sim::{Effect, Intent, MoveOutcome};

    pub struct Rig {
        pub world: World,
        pub walk: Walker,
        pub action: Action,
        pub biology: Biology,
        pub inventory: Inventory,
        /// What the last tick's [`Task::execute`] asked of the world beyond
        /// this unit — reset to [`Effect::None`] at the start of every
        /// [`Rig::tick`], the same as the real brain's own local copy.
        pub effect: Effect,
    }

    impl Rig {
        pub fn new(map: Map, cell: Point) -> Rig {
            Rig {
                world: World::new(map),
                walk: Walker::new(Uid::new(EntityType::Human, 9), cell, 2.0),
                action: Action::None,
                biology: Biology::new(Stats::calm()),
                inventory: Inventory::human(),
                effect: Effect::None,
            }
        }

        /// What is in the rig's hand, which is what the tasks that take and
        /// consume things work on.
        pub fn hand(&self) -> Option<ItemKind> {
            self.inventory.hand()
        }

        /// Put something in it, or empty it.
        pub fn set_hand(&mut self, item: Option<ItemKind>) {
            let _ = self.inventory.set_hand(item);
        }

        /// One tick: walk whatever route there is (on open floor, nothing
        /// refuses a step), then run the task.
        pub fn tick(&mut self, task: &mut Task) -> TaskResult {
            let Rig {
                world,
                walk,
                action,
                biology,
                inventory,
                effect,
            } = self;
            world.tick += 1;
            let think = world.ctx();
            let intent = walk.think(&think);
            walk.apply(&intent);
            let outcome = match intent {
                Intent::Idle => MoveOutcome::Idle,
                Intent::Move { .. } => MoveOutcome::Moved,
            };
            *effect = Effect::None;
            task.execute(&mut TaskCtx {
                think: &think,
                outcome,
                walk,
                action,
                biology: Some(biology),
                inventory: Some(inventory),
                effect,
            })
        }

        /// Tick until the task ends, or give up after `ticks`.
        pub fn run(&mut self, task: &mut Task, ticks: u32) -> TaskResult {
            let mut result = TaskResult::InProgress;
            for _ in 0..ticks {
                result = self.tick(task);
                if result.is_finished() {
                    break;
                }
            }
            result
        }
    }
}
