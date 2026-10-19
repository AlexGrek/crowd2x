//! [`CloseFridge`]: close a fridge's door, from beside it — the mirror of
//! [`super::OpenFridge`].

use crate::map::Point;
use crate::save::pb;
use crate::sim::clock::watched;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Five world seconds to close the door — the same as opening it.
pub const CLOSE_SECONDS: f32 = watched(5.0);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CloseFridge {
    /// The fridge's own cell.
    pub fridge: Point,
    pub seconds: f32,
}

impl TaskExecutor for CloseFridge {
    /// **Only from one step away, checked every tick.** Already closed
    /// succeeds without starting an action: a fridge somebody else beat this
    /// unit to closing is not closed twice.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.fridge) > 1 {
            return TaskResult::Failed;
        }
        if !ctx.think.fridges.is_open(self.fridge) {
            return TaskResult::Success;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.fridge, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let at = self.fridge;
                *ctx.effect = Effect::Fridge { at, open: false };
                ctx.think.log.push(format!(
                    "{} closed the fridge at {}, {} ({:.1}°C)",
                    ctx.walk.body().uid(),
                    at.x,
                    at.y,
                    ctx.think.fridges.temperature(at)
                ));
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("close the fridge at {}, {}", self.fridge.x, self.fridge.y)
    }
}

impl Task {
    /// Close the fridge in `fridge`, from a cell beside it, unless it is
    /// closed already.
    pub const fn close_fridge(fridge: Point, seconds: f32) -> Task {
        Task::CloseFridge(CloseFridge { fridge, seconds })
    }
}

/// Saved as `close fridge`: the fridge's cell and the seconds.
impl TimedTask for CloseFridge {
    const NAME: &'static str = "close fridge";

    fn new(cell: Point, seconds: f32) -> CloseFridge {
        CloseFridge { fridge: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.fridge
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }

    fn legacy(saved: &pb::TaskKind) -> Option<&pb::Timed> {
        match saved {
            pb::TaskKind::CloseFridge(task) => Some(task),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::super::super::task::Task;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::testing::prop_at;

    fn rig_at(fridge: Point, cell: Point) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        prop_at(&mut map, "fridge", fridge);
        Rig::new(map, cell)
    }

    #[test]
    fn closing_from_more_than_a_step_away_fails_without_starting() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_at(fridge, Point::new(1, 1));
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::close_fridge(fridge, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.effect, Effect::None);
        assert!(rig.world.fridges.is_open(fridge), "should not have been touched");
    }

    #[test]
    fn the_effect_is_only_emitted_once_the_door_has_finished_closing() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_at(fridge, Point::new(4, 5));
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::close_fridge(fridge, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None, "not yet: the door is still swinging");

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Fridge { at: fridge, open: false });
    }

    #[test]
    fn closing_a_fridge_that_is_already_closed_succeeds_at_once_with_no_effect() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_at(fridge, Point::new(4, 5));
        let mut task = Task::close_fridge(fridge, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Success);
        assert!(rig.action.is_none());
        assert_eq!(rig.effect, Effect::None);
    }
}
