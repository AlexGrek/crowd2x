//! A dog emptying its bowel at the cell it stands in. The world leaves the
//! mess ([`Effect::Defecate`]) and the dog its relief ([`GameEntity::defecate`]).
//!
//! [`GameEntity::defecate`]: crate::sim::GameEntity::defecate

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::Effect;

/// A minute of world to do its business.
pub const DEFECATE_SECONDS: f32 = watched(MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Defecate {
    pub at: Point,
    pub seconds: f32,
}

impl TaskExecutor for Defecate {
    /// Only where it stands: choosing the spot and walking to it are the
    /// goal's and a walk's.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here() != self.at || !ctx.think.is_passable(self.at) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.at, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::Defecate { at: self.at };
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("do its business at {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    pub const fn defecate(at: Point, seconds: f32) -> Task {
        Task::Defecate(Defecate { at, seconds })
    }
}

impl TimedTask for Defecate {
    const NAME: &'static str = "defecate";
    fn new(at: Point, seconds: f32) -> Self {
        Self { at, seconds }
    }
    fn cell(&self) -> Point {
        self.at
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
    fn it_squats_where_it_stands_and_asks_the_world_for_the_mess() {
        let here = Point::new(4, 4);
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), here);
        let mut task = Task::defecate(here, 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None, "not until it is done");
        assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Defecate { at: here });
    }

    #[test]
    fn it_fails_from_anywhere_but_the_cell_chosen() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), Point::new(4, 4));
        let mut task = Task::defecate(Point::new(5, 4), 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.effect, Effect::None);
    }
}
