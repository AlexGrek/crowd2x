//! [`OpenFridge`]: open a fridge's door, from beside it.

use crate::map::Point;
use crate::sim::clock::watched;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

/// Five world seconds to open the door — a duration a body is *watched*
/// standing through, so written and converted like every other one (see
/// [`crate::sim::clock`]).
pub const OPEN_SECONDS: f32 = watched(5.0);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OpenFridge {
    /// The fridge's own cell.
    pub fridge: Point,
    pub seconds: f32,
}

impl TaskExecutor for OpenFridge {
    /// **Only from one step away, checked every tick.** Already open —
    /// somebody else's doing, or this unit's own left ajar — succeeds without
    /// starting an action at all: nothing asks for a second open of an open
    /// door.
    ///
    /// Finishing sets [`Effect::Fridge`] for [`super::super::super::world_step`](crate::sim::world_step)
    /// to grant, and logs the temperature at the moment the door swung open —
    /// read off `ctx.think.fridges`, which is the fridge as it stood at the
    /// *start* of this tick, one tick behind whatever `world_step` is about
    /// to make of this very effect.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.fridge) > 1 {
            return TaskResult::Failed;
        }
        if ctx.think.fridges.is_open(self.fridge) {
            return TaskResult::Success;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.fridge, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let at = self.fridge;
                *ctx.effect = Effect::Fridge { at, open: true };
                ctx.think.log.push(format!(
                    "{} opened the fridge at {}, {} ({:.1}°C)",
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
        format!("open the fridge at {}, {}", self.fridge.x, self.fridge.y)
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
    fn opening_from_more_than_a_step_away_fails_without_starting() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_at(fridge, Point::new(1, 1));
        let mut task = Task::open_fridge(fridge, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.effect, Effect::None);
        assert!(!rig.world.fridges.is_open(fridge));
    }

    #[test]
    fn the_effect_is_only_emitted_once_the_door_has_finished_opening() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_at(fridge, Point::new(4, 5));
        let mut task = Task::open_fridge(fridge, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None, "not yet: the door is still swinging");

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Fridge { at: fridge, open: true });
    }

    #[test]
    fn opening_a_fridge_that_is_already_open_succeeds_at_once_with_no_effect() {
        let fridge = Point::new(5, 5);
        let mut rig = rig_at(fridge, Point::new(4, 5));
        assert!(rig.world.fridges.set_open(fridge, true));
        let mut task = Task::open_fridge(fridge, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Success);
        assert!(rig.action.is_none());
        assert_eq!(rig.effect, Effect::None);
    }
}
