//! The task executors that exist: how each small step is carried out.
//!
//! Each is a `Copy` struct holding what the step is about, with an
//! [`TaskExecutor`](super::task::TaskExecutor) impl that starts its action,
//! checks what has to stay true while it runs, and applies what finishing it
//! means. See [`super::task`] for why they are not boxed.

pub mod consume_item;
pub mod move_to;
pub mod take_item;
pub mod use_toilet;
pub mod wait;

pub use consume_item::ConsumeItem;
pub use move_to::MoveTo;
pub use take_item::TakeItem;
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
    use crate::sim::item::ItemKind;
    use crate::sim::testing::World;
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::walker::Walker;
    use crate::sim::{Intent, MoveOutcome};

    pub struct Rig {
        pub world: World,
        pub walk: Walker,
        pub action: Action,
        pub biology: Biology,
        pub carried: Option<ItemKind>,
    }

    impl Rig {
        pub fn new(map: Map, cell: Point) -> Rig {
            Rig {
                world: World::new(map),
                walk: Walker::new(Uid::new(EntityType::Human, 9), cell, 2.0),
                action: Action::None,
                biology: Biology::new(Stats::calm()),
                carried: None,
            }
        }

        /// One tick: walk whatever route there is (on open floor, nothing
        /// refuses a step), then run the task.
        pub fn tick(&mut self, task: &mut Task) -> TaskResult {
            let Rig {
                world,
                walk,
                action,
                biology,
                carried,
            } = self;
            world.tick += 1;
            let think = world.ctx();
            let intent = walk.think(&think);
            walk.apply(&intent);
            let outcome = match intent {
                Intent::Idle => MoveOutcome::Idle,
                Intent::Move { .. } => MoveOutcome::Moved,
            };
            task.execute(&mut TaskCtx {
                think: &think,
                outcome,
                walk,
                action,
                biology: Some(biology),
                carried: Some(carried),
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
