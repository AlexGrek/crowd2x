//! [`UseToilet`]: use a toilet, from beside it.

use crate::map::Point;
use crate::sim::biology::Event;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct UseToilet {
    /// The toilet's cell.
    pub toilet: Point,
    /// How long using it takes.
    pub seconds: f32,
}

impl TaskExecutor for UseToilet {
    /// **Only from one step away, checked every tick**, like
    /// [`TakeItem`](super::TakeItem): the walk there is a task of its own. A
    /// kind with no body has nothing to relieve.
    ///
    /// The body hears [`Event::Relieved`] only once the action is over — a unit
    /// pulled away halfway is exactly as full as it was.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.toilet) > 1 || ctx.biology.is_none() {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.toilet, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Relieved);
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("use the toilet at {}, {}", self.toilet.x, self.toilet.y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::super::super::task::Task;
    use super::*;
    use crate::map::{Map, Size, FLOOR};

    fn rig_at(cell: Point) -> Rig {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), cell);
        rig.biology.edit(|stats| stats.with_bladder(90.0));
        rig
    }

    #[test]
    fn using_a_toilet_from_across_the_room_fails_without_starting() {
        let mut rig = rig_at(Point::new(1, 1));
        let mut task = Task::use_toilet(Point::new(6, 6), 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.biology.stats().bladder(), 90.0);
    }

    #[test]
    fn the_bladder_is_emptied_only_once_the_toilet_has_been_used() {
        let mut rig = rig_at(Point::new(5, 6));
        let mut task = Task::use_toilet(Point::new(6, 6), 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.biology.stats().bladder(), 90.0, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.biology.stats().bladder(), 0.0);
    }

    #[test]
    fn walking_away_mid_use_fails_it_and_relieves_nothing() {
        let mut rig = rig_at(Point::new(5, 6));
        let mut task = Task::use_toilet(Point::new(6, 6), 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);

        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().bladder(), 90.0);
    }
}
