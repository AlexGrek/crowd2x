//! [`MoveTo`]: walk until standing in the middle of a cell.

use crate::map::Point;

use super::super::action::Action;
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MoveTo {
    pub cell: Point,
    pub beside: bool,
}

impl TaskExecutor for MoveTo {
    /// The route is asked for once, when the walk starts. No way there fails
    /// the task on the spot; somebody in the way with no local way round fails
    /// it later — and the goal tells the two apart by whether a body refused
    /// the step.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.action.is_none() {
            let destinations = if self.beside {
                crate::sim::path::Destinations::from_cells(
                    self.cell
                        .cardinal_neighbours()
                        .into_iter()
                        .filter(|&c| ctx.think.is_passable_with(c, ctx.walk.body().home())),
                )
            } else {
                crate::sim::path::Destinations::one(self.cell)
            };
            return match Action::walk_to_any(ctx.walk, ctx.think, destinations) {
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
        format!(
            "move {} {}, {}",
            if self.beside { "beside" } else { "to" },
            self.cell.x,
            self.cell.y
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{FLOOR, Map, Size};

    #[test]
    fn a_feature_approach_excludes_a_door_locked_to_this_unit() {
        let map = crate::sim::property::tests::house();
        let world = crate::sim::testing::World::new(map.clone());
        assert!(!world.ctx().is_passable_with(Point::new(5, 2), None));
        let mut rig = Rig::new(map, Point::new(5, 0));
        let mut task = Task::move_beside(Point::new(5, 1));
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_ne!(rig.walk.destination(), Some(Point::new(5, 2)));
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
    }

    #[test]
    fn a_move_to_task_succeeds_standing_in_the_middle_of_its_cell() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), Point::new(1, 1));
        let mut task = Task::move_to(Point::new(5, 3));

        assert_eq!(
            rig.tick(&mut task),
            TaskResult::Executing,
            "the walk starts at once"
        );
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        let (x, y) = rig.walk.body().position();
        assert!(
            (x - 5.5).abs() < 1e-3 && (y - 3.5).abs() < 1e-3,
            "stopped at {x}, {y}"
        );
    }

    #[test]
    fn a_move_to_with_no_way_there_fails_without_an_action() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), Point::new(1, 1));
        let mut task = Task::move_to(Point::new(40, 40));

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }
}
