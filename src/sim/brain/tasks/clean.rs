//! A cleaner mopping up one dirty cell of the district, from on it or beside
//! it. The world takes the dirt away ([`Effect::Clean`]).

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::Effect;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{MINUTE, TIME_SCALE, watched};
use crate::sim::dirt::Dirt;

/// Five world minutes a cell: a mop and a bucket, a stain at a time.
pub const CLEAN_SECONDS: f32 = watched(5.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Clean {
    pub at: Point,
    pub seconds: f32,
}

impl TaskExecutor for Clean {
    /// Fails from further than a step away, and once there is nothing left
    /// to clean — somebody else got there first. A cell that only dried below
    /// worth cleaning while it was being mopped is mopped to the end.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.at) > 1
            || ctx.think.dirt.at(self.at) == Dirt::CLEAN
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
                *ctx.effect = Effect::Clean { at: self.at };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before the cell was clean: it is as dirty as it was, but
    /// the time spent was still work.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("clean up {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    pub const fn clean(at: Point, seconds: f32) -> Task {
        Task::Clean(Clean { at, seconds })
    }
}

impl TimedTask for Clean {
    const NAME: &'static str = "clean";
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

    const STAIN: Point = Point::new(3, 3);

    fn rig_beside_a_stain() -> Rig {
        let mut rig = Rig::new(Map::new(Size::new(7, 7), FLOOR), Point::new(3, 2));
        rig.biology.edit(|stats| stats.with_stamina(80.0).with_fun(40.0));
        assert!(rig.world.facilities.dirt.set(STAIN, Dirt { dirt: 0.3, blood: 0.2, ..Dirt::CLEAN }));
        rig
    }

    #[test]
    fn a_stain_is_asked_away_once_it_is_mopped_and_not_before() {
        let mut rig = rig_beside_a_stain();
        let mut task = Task::clean(STAIN, CLEAN_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.action.interacting_with(), Some(STAIN));
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.run(&mut task, 10_000), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Clean { at: STAIN });
        assert_ne!(rig.world.dirt.at(STAIN), Dirt::CLEAN, "the world takes it away, not the task");
        assert!(rig.biology.stats().stamina() < 80.0, "it is work");
        assert_eq!(rig.biology.stats().fun(), 40.0, "and no fun");
    }

    #[test]
    fn not_from_across_the_room_and_not_once_somebody_else_has_cleaned_it() {
        let mut far = rig_beside_a_stain();
        far.walk.body_mut().set_position((0.5, 0.5));
        assert_eq!(far.tick(&mut Task::clean(STAIN, CLEAN_SECONDS)), TaskResult::Failed, "too far");

        let mut rig = rig_beside_a_stain();
        let mut task = Task::clean(STAIN, CLEAN_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert!(rig.world.facilities.dirt.set(STAIN, Dirt::CLEAN));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed, "nothing left to clean");
        assert_eq!(rig.effect, Effect::None);

        let mut on = rig_beside_a_stain();
        on.walk.body_mut().set_position((3.5, 3.5));
        assert_eq!(on.tick(&mut Task::clean(STAIN, CLEAN_SECONDS)), TaskResult::Executing, "standing in it will do");
    }
}
