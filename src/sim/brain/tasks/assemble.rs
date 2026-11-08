//! [`Assemble`]: an engineer's hour of work on the robot being built, beside
//! the robot factory's assembly bay. The world counts it
//! ([`Effect::Assemble`](crate::sim::Effect::Assemble)): the first hour takes
//! the robot's steel off the parts rack, and the hour that makes up
//! [`ASSEMBLY_HOURS`](crate::sim::robots::ASSEMBLY_HOURS) brings the robot off
//! the line. An hour cut short counts for nothing.

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, HOUR, TIME_SCALE};
use crate::sim::feature::FeatureKind;
use crate::sim::Effect;

pub const ASSEMBLE_SECONDS: f32 = watched(HOUR);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Assemble {
    /// The assembly bay.
    pub bay: Point,
    pub seconds: f32,
}

impl TaskExecutor for Assemble {
    /// From beside the bay, every tick.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.bay) > 1 || !ctx.think.features.has(FeatureKind::RepairBay, self.bay) || ctx.biology.is_none() {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.bay, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let world_seconds = self.seconds * TIME_SCALE;
                *ctx.effect = Effect::Assemble { at: self.bay, seconds: world_seconds, when: ctx.think.clock };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds });
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
        format!("build a robot at the assembly bay at {}, {}", self.bay.x, self.bay.y)
    }
}

impl Task {
    /// Work on the robot being built, beside the bay in `bay`, for `seconds`.
    pub const fn assemble(bay: Point, seconds: f32) -> Task {
        Task::Assemble(Assemble { bay, seconds })
    }
}

impl TimedTask for Assemble {
    const NAME: &'static str = "assemble";
    fn new(bay: Point, seconds: f32) -> Self {
        Self { bay, seconds }
    }
    fn cell(&self) -> Point {
        self.bay
    }
    fn seconds(&self) -> f32 {
        self.seconds
    }
}
