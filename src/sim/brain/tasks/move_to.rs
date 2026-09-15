//! [`MoveTo`]: walk until standing in the middle of a cell.

use crate::map::Point;

use super::super::action::Action;
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MoveTo {
    pub cell: Point,
}

impl TaskExecutor for MoveTo {
    /// The route is asked for once, when the walk starts. No way there fails
    /// the task on the spot; somebody in the way with no local way round fails
    /// it later — and the goal tells the two apart by whether a body refused
    /// the step.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.action.is_none() {
            return match Action::walk_to(ctx.walk, ctx.think, self.cell) {
                Some(walk) => {
                    *ctx.action = walk;
                    TaskResult::Executing
                }
                None => TaskResult::Failed,
            };
        }
        TaskResult::of(ctx.advance_action())
    }

    fn describe(&self) -> String {
        format!("move to {}, {}", self.cell.x, self.cell.y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::super::super::task::Task;
    use super::*;
    use crate::map::{Map, Size, FLOOR};

    #[test]
    fn a_move_to_task_succeeds_standing_in_the_middle_of_its_cell() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), Point::new(1, 1));
        let mut task = Task::move_to(Point::new(5, 3));

        assert_eq!(rig.tick(&mut task), TaskResult::Executing, "the walk starts at once");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        let (x, y) = rig.walk.body().position();
        assert!((x - 5.5).abs() < 1e-3 && (y - 3.5).abs() < 1e-3, "stopped at {x}, {y}");
    }

    #[test]
    fn a_move_to_with_no_way_there_fails_without_an_action() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), Point::new(1, 1));
        let mut task = Task::move_to(Point::new(40, 40));

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }
}
