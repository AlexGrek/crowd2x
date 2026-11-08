//! [`Overhaul`]: an engineer's hour beside a worn robot waiting at the robot
//! factory's bay — whole again. The world settles it
//! ([`Effect::Overhaul`](crate::sim::Effect::Overhaul)) and the robot feels it
//! in its own react (`sim::robots`).

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, HOUR, TIME_SCALE};
use crate::sim::Effect;

pub const OVERHAUL_SECONDS: f32 = watched(HOUR);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Overhaul {
    /// Where the robot stands, waiting.
    pub at: Point,
    pub seconds: f32,
}

impl TaskExecutor for Overhaul {
    /// From beside the robot, every tick, and a robot still waiting there.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let waiting = ctx.think.robots.waiting().iter().any(|&(_, cell)| cell == self.at);
        if ctx.here().manhattan_distance(self.at) > 1 || !waiting || ctx.biology.is_none() {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.at, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::Overhaul { at: self.at };
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
        format!("overhaul the robot at {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    /// Overhaul the robot waiting in `at`, from beside it, for `seconds`.
    pub const fn overhaul(at: Point, seconds: f32) -> Task {
        Task::Overhaul(Overhaul { at, seconds })
    }
}

impl TimedTask for Overhaul {
    const NAME: &'static str = "overhaul";
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
