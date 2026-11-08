//! [`FixRobot`]: an electrician's quarter of an hour beside a robot standing
//! stopped — a flat one recharged, a broken one mended. The world settles
//! which ([`Effect::FixRobot`](crate::sim::Effect::FixRobot)), and the robot
//! feels it in its own react (`sim::robots`).

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::Effect;

pub const FIX_ROBOT_SECONDS: f32 = watched(15.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FixRobot {
    /// Where the robot stands.
    pub at: Point,
    pub seconds: f32,
}

impl TaskExecutor for FixRobot {
    /// From beside the robot, every tick, and the robot still stopped there.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.at) > 1 || ctx.think.robots.stopped_at(self.at).is_none() || ctx.biology.is_none() {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.at, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::FixRobot { at: self.at };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("fix the robot at {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    /// Fix the robot stopped in `at`, from beside it, for `seconds`.
    pub const fn fix_robot(at: Point, seconds: f32) -> Task {
        Task::FixRobot(FixRobot { at, seconds })
    }
}

impl TimedTask for FixRobot {
    const NAME: &'static str = "fix robot";
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
