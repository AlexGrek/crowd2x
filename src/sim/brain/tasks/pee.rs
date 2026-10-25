//! Relieve at a chosen outdoor spot. Walking there is a separate task.

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::Effect;
use crate::sim::biology::ProcessId;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pee {
    pub at: Point,
    pub seconds: f32,
}

impl TaskExecutor for Pee {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here() != self.at
            || ctx.think.map.site().contains(self.at)
            || !ctx.think.is_passable(self.at)
            || !ctx
                .biology
                .as_ref()
                .is_some_and(|b| b.is_running(ProcessId::Bladder))
        {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.at, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::Pee { at: self.at };
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("do toilet outside at {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    pub const fn pee(at: Point, seconds: f32) -> Self {
        Self::Pee(Pee { at, seconds })
    }
}

impl TimedTask for Pee {
    const NAME: &'static str = "pee outside";
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
    use super::*;
    use crate::map::{FLOOR, Map, Size};
    use crate::sim::brain::tasks::rig::Rig;

    #[test]
    fn peeing_requires_the_chosen_outdoor_cell_through_completion() {
        let outside = Point::new(-6, 6);
        let map = Map::new(Size::new(10, 10), FLOOR).with_wasteland(5);
        let mut rig = Rig::new(map, outside);
        let mut task = Task::pee(outside, 0.1);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        rig.walk.body_mut().set_position((-4.5, 6.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.effect, Effect::None);
        let inside = Point::new(2, 2);
        let mut rig = Rig::new(Map::new(Size::new(10, 10), FLOOR), inside);
        assert_eq!(rig.tick(&mut Task::pee(inside, 0.1)), TaskResult::Failed);
    }

    #[test]
    fn peeing_emits_its_effect_only_when_finished_and_refuses_a_stopped_bladder() {
        let cell = Point::new(-6, 6);
        let mut rig = Rig::new(Map::new(Size::new(10, 10), FLOOR).with_wasteland(5), cell);
        let mut task = Task::pee(cell, 0.1);
        assert_eq!(rig.run(&mut task, 20), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Pee { at: cell });
        rig.action = Action::None;
        rig.biology.set_running(ProcessId::Bladder, false);
        assert_eq!(rig.tick(&mut Task::pee(cell, 0.1)), TaskResult::Failed);
        assert_eq!(rig.effect, Effect::None);
    }
}
