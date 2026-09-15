//! [`Wait`]: stand still for a while.

use super::super::action::Action;
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

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
