//! A pet sleeps on the ground near its sleeping owner, without claiming a bed.

use super::super::action::Action;
use super::super::goals::follow::CLOSE;
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DogSleep {
    pub cell: Point,
    pub seconds: f32,
}

impl TaskExecutor for DogSleep {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let owner = ctx.think.bonds.partner(ctx.walk.body().uid());
        if ctx.here() != self.cell
            || !owner.is_some_and(|owner| {
                owner.asleep && self.cell.chebyshev_distance(owner.cell) <= CLOSE
            })
        {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.cell, self.seconds);
            return TaskResult::Executing;
        }
        TaskResult::of(ctx.advance_action())
    }

    fn describe(&self) -> String {
        format!("sleep beside the owner at {}, {}", self.cell.x, self.cell.y)
    }
}

impl Task {
    pub const fn dog_sleep(cell: Point, seconds: f32) -> Task {
        Task::DogSleep(DogSleep { cell, seconds })
    }
}

impl TimedTask for DogSleep {
    const NAME: &'static str = "dog_sleep";
    fn new(cell: Point, seconds: f32) -> Self {
        Self { cell, seconds }
    }
    fn cell(&self) -> Point {
        self.cell
    }
    fn seconds(&self) -> f32 {
        self.seconds
    }
}
