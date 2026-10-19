//! [`Wait`]: stand still for a while.

use crate::save::{pb, Invalid};

use super::super::action::Action;
use super::super::task::{self, SavedTask, Task, TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Wait {
    pub seconds: f32,
}

impl TaskExecutor for Wait {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.action.is_none() {
            *ctx.action = Action::wait(self.seconds);
            return TaskResult::Executing;
        }
        TaskResult::of(ctx.advance_action())
    }

    fn describe(&self) -> String {
        format!("wait {:.1}s", self.seconds)
    }
}

impl Task {
    /// Stand still for a while.
    pub const fn wait(seconds: f32) -> Task {
        Task::Wait(Wait { seconds })
    }
}

/// Saved as `wait`, with no cell.
impl SavedTask for Wait {
    fn save(&self) -> pb::TaskKind {
        task::named("wait", None, self.seconds)
    }

    fn restore(saved: &pb::TaskKind) -> Option<Result<Wait, Invalid>> {
        let seconds = match saved {
            pb::TaskKind::Wait(task) => task.seconds,
            _ => task::read_named(saved, "wait")?.1,
        };
        Some(task::seconds(seconds).map(|seconds| Wait { seconds }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::super::super::task::Task;
    use super::*;
    use crate::map::{Map, Point, Size, FLOOR};

    #[test]
    fn a_wait_is_executing_until_its_time_is_up() {
        let mut rig = Rig::new(Map::new(Size::new(3, 3), FLOOR), Point::new(1, 1));
        rig.world.dt = 0.25;
        let mut task = Task::wait(1.0);

        // One tick to start, four to count a second at a quarter each.
        for _ in 0..4 {
            assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        }
        assert_eq!(rig.tick(&mut task), TaskResult::Success);
    }
}
