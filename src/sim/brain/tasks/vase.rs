//! Short furniture interactions. Shared transactions commit in the world step.
use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::{Effect, ItemKind};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BuyVase {
    pub at: Point,
    pub seconds: f32,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlaceVase {
    pub at: Point,
    pub seconds: f32,
}
impl TaskExecutor for BuyVase {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.at) > 1
            || !ctx
                .think
                .features
                .has(crate::sim::FeatureKind::DecorSection, self.at)
            || !ctx
                .inventory
                .as_deref()
                .is_some_and(|p| p.room_for(ItemKind::FlowerVase))
        {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.at, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::BuyVase {
                    at: self.at,
                    clock: ctx.think.clock,
                };
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }
    fn describe(&self) -> String {
        "buy a flower vase".to_string()
    }
}
impl TaskExecutor for PlaceVase {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.at) != 1
            || ctx
                .think
                .decor
                .receipt(ctx.walk.body().uid())
                .is_none_or(|r| r.cell != self.at)
            || !ctx
                .inventory
                .as_deref()
                .is_some_and(|p| p.count(ItemKind::FlowerVase) > 0)
        {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.at, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::PlaceVase { at: self.at };
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }
    fn describe(&self) -> String {
        "place a flower vase".to_string()
    }
}
impl Task {
    pub const fn buy_vase(at: Point, seconds: f32) -> Task {
        Task::BuyVase(BuyVase { at, seconds })
    }
    pub const fn place_vase(at: Point, seconds: f32) -> Task {
        Task::PlaceVase(PlaceVase { at, seconds })
    }
}
impl TimedTask for BuyVase {
    const NAME: &'static str = "buy a flower vase";
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
impl TimedTask for PlaceVase {
    const NAME: &'static str = "place a flower vase";
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
