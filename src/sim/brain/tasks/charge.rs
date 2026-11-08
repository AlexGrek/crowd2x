//! [`Charge`]: stand beside a charging dock and charge there for a while.
//!
//! A place and a time, like any timed task. The task changes nothing itself:
//! while it is current with its action going, the robot's battery fills
//! (`Brain::is_charging`, `Robot::react`), and the routine that sent it lets
//! go once the battery is full, which ends the stretch early. A dock with no
//! power is no charging dock (`Features`), so the stretch fails.

use crate::map::Point;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// One stretch at the dock: half an hour of world, thirty points of charge.
pub const CHARGE_SECONDS: f32 = watched(30.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Charge {
    /// The dock charged at.
    pub dock: Point,
    pub seconds: f32,
}

impl TaskExecutor for Charge {
    /// **From beside the dock, every tick**, and the dock powered.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.dock) > 1 || !ctx.think.features.has(FeatureKind::Charger, self.dock) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.dock, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => TaskResult::Success,
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("charge at the dock at {}, {}", self.dock.x, self.dock.y)
    }
}

impl Task {
    /// Charge beside the dock in `dock` for `seconds`.
    pub const fn charge(dock: Point, seconds: f32) -> Task {
        Task::Charge(Charge { dock, seconds })
    }
}

/// Saved as `charge`: the dock and the seconds.
impl TimedTask for Charge {
    const NAME: &'static str = "charge";

    fn new(cell: Point, seconds: f32) -> Charge {
        Charge { dock: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.dock
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
    use crate::sim::testing::prop_at;

    const DOCK: Point = Point::new(4, 4);

    fn station() -> Map {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        prop_at(&mut map, "charging dock", DOCK);
        map
    }

    #[test]
    fn charging_from_across_the_room_fails_and_beside_the_dock_runs_its_stretch() {
        let mut far = Rig::new(station(), Point::new(1, 1));
        assert_eq!(far.tick(&mut Task::charge(DOCK, 1.0)), TaskResult::Failed);
        let mut beside = Rig::new(station(), DOCK.offset(0, -1));
        let mut task = Task::charge(DOCK, 1.0);
        assert_eq!(beside.tick(&mut task), TaskResult::Executing);
        assert!(matches!(beside.action, Action::Interact { .. }), "plugged in");
        assert_eq!(beside.run(&mut task, 200), TaskResult::Success);
    }
}
