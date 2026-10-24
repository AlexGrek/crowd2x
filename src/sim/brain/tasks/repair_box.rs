//! A timed repair, from beside a damaged box. The world settles the effect.

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::Effect;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{MINUTE, TIME_SCALE, watched};

pub const REPAIR_SECONDS: f32 = watched(15.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RepairBox {
    pub at: Point,
    pub seconds: f32,
}

impl TaskExecutor for RepairBox {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.at) > 1
            || !ctx.think.electrical.is_damaged(self.at)
            || ctx.biology.is_none()
        {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.at, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::RepairBox { at: self.at };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked {
                        world_seconds: self.seconds * TIME_SCALE,
                    });
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
            biology.handle(Event::Worked {
                world_seconds: elapsed * TIME_SCALE,
            });
        }
    }

    fn describe(&self) -> String {
        format!("repair box at {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    pub const fn repair_box(at: Point, seconds: f32) -> Task {
        Task::RepairBox(RepairBox { at, seconds })
    }
}

impl TimedTask for RepairBox {
    const NAME: &'static str = "repair box";
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
    use crate::map::{FLOOR, Map, Size};

    #[test]
    fn repairs_check_distance_and_damage_through_completion_and_only_finish_once() {
        let at = Point::new(3, 3);
        let mut rig = Rig::new(Map::new(Size::new(7, 7), FLOOR), Point::new(3, 2));
        rig.world.facilities.electrical.damage(at);
        let mut task = Task::repair_box(at, 0.1);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert_eq!(rig.effect, Effect::RepairBox { at });

        rig.action = Action::None;
        let mut task = Task::repair_box(at, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.world.facilities.electrical.repair(at);
        assert_eq!(
            rig.tick(&mut task),
            TaskResult::Failed,
            "another electrician repaired it"
        );
        assert_eq!(rig.effect, Effect::None);
        rig.world.facilities.electrical.damage(at);
        rig.walk.body_mut().set_position((0.5, 0.5));
        assert_eq!(
            rig.tick(&mut task),
            TaskResult::Failed,
            "pulled away mid-repair"
        );
        assert_eq!(rig.effect, Effect::None);
    }
}
