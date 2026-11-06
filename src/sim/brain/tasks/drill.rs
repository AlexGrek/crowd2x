//! [`Drill`]: drill on the training ground — walk to a cell of it and, if the
//! drill says so, stand there for a while.
//!
//! A soldier's circles are a string of these with no standing (`seconds` 0): a
//! step from one waypoint of the lap to the next ([`next_waypoint`]
//! (crate::sim::training::next_waypoint)). The commander's is one of them
//! with an hour of standing, in the middle of the ground. Every tick of either
//! is work: it tires and is no fun, as an hour at a desk does, so seven hours
//! of drill wear a body the way seven of a shift do.

use crate::map::Point;
use crate::sim::biology::Event;
use crate::sim::clock::TIME_SCALE;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Drill {
    /// Where on the ground: a waypoint, or the middle.
    pub cell: Point,
    /// How long to stand there once arrived, in **watched** seconds; 0 to go
    /// straight on.
    pub seconds: f32,
}

impl TaskExecutor for Drill {
    /// The route is asked for once, when the walk starts, and no way there
    /// fails the task. Every tick that follows tires, in the walk and in the
    /// standing alike.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.biology.is_none() {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            if ctx.here() == self.cell {
                if self.seconds <= 0.0 {
                    return TaskResult::Success;
                }
                *ctx.action = Action::wait(self.seconds);
                return TaskResult::Executing;
            }
            return match Action::walk_to(ctx.walk, ctx.think, self.cell) {
                Some(walk) => {
                    *ctx.action = walk;
                    TaskResult::Executing
                }
                None => TaskResult::Failed,
            };
        }
        let walking = matches!(ctx.action, Action::Move { .. });
        let state = ctx.advance_action();
        if let Some(biology) = ctx.biology.as_deref_mut() {
            biology.handle(Event::Worked { world_seconds: ctx.think.dt * TIME_SCALE });
        }
        match state {
            ActionState::Finished if walking && self.seconds > 0.0 => {
                *ctx.action = Action::wait(self.seconds);
                TaskResult::Executing
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        if self.seconds > 0.0 {
            format!("stand at the drill at {}, {}", self.cell.x, self.cell.y)
        } else {
            format!("march to {}, {}", self.cell.x, self.cell.y)
        }
    }
}

impl Task {
    /// Walk to `cell` of the training ground and stand there for `seconds`
    /// (0: no standing).
    pub const fn drill(cell: Point, seconds: f32) -> Task {
        Task::Drill(Drill { cell, seconds })
    }
}

impl TimedTask for Drill {
    const NAME: &'static str = "drill";

    fn new(cell: Point, seconds: f32) -> Self {
        Drill { cell, seconds }
    }

    fn cell(&self) -> Point {
        self.cell
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};

    #[test]
    fn a_march_walks_to_its_waypoint_and_tires_on_the_way() {
        let mut rig = Rig::new(Map::new(Size::new(12, 12), FLOOR), Point::new(2, 2));
        let stamina = rig.biology.stats().stamina();
        let mut task = Task::drill(Point::new(6, 2), 0.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.walk.body().center_position(), Point::new(6, 2));
        assert!(rig.biology.stats().stamina() < stamina, "marching is work");
    }

    #[test]
    fn standing_in_the_middle_lasts_its_time_and_tires() {
        let mut rig = Rig::new(Map::new(Size::new(12, 12), FLOOR), Point::new(5, 5));
        let stamina = rig.biology.stats().stamina();
        rig.world.dt = 0.25;
        let mut task = Task::drill(Point::new(5, 5), 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert!(matches!(rig.action, Action::Wait { .. }), "already there: it only stands");
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert!(rig.biology.stats().stamina() < stamina);
    }

    #[test]
    fn a_ground_with_no_way_there_fails_the_march() {
        let mut rig = Rig::new(Map::new(Size::new(12, 12), FLOOR), Point::new(2, 2));
        assert_eq!(rig.tick(&mut Task::drill(Point::new(90, 90), 0.0)), TaskResult::Failed);
    }
}
