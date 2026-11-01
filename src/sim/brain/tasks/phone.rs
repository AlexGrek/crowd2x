//! [`Phone`]: stand where one is and look at a mobile phone.
//!
//! A place and a time, like any timed task — the place being the cell the unit
//! stands in, kept for the log and the save. Nothing is used up and nothing is
//! walked to: everybody has a phone. **What is on it is the brain's**, once
//! the look is over (`sim::news::read`, from `Brain::react`, which holds the
//! memory of what was read): the next article not yet read, or with none a
//! little fun ([`Event::Phoned`](crate::sim::biology::Event::Phoned)). A look
//! cut short reads nothing and is no fun at all.

use crate::map::Point;
use crate::sim::clock::{watched, MINUTE};

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Four world minutes with one's eyes on a phone.
pub const PHONE_SECONDS: f32 = watched(4.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Phone {
    /// Where the unit stood when it took the phone out.
    pub at: Point,
    /// How long the look takes.
    pub seconds: f32,
}

impl TaskExecutor for Phone {
    /// **Only where it began, checked every tick**: a unit pushed or walked
    /// away has put its phone back in its pocket.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.biology.is_none() || ctx.here() != self.at {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::wait(self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => TaskResult::Success,
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("look at the phone at {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    /// Look at a phone for `seconds`, standing in `at`.
    pub const fn phone(at: Point, seconds: f32) -> Task {
        Task::Phone(Phone { at, seconds })
    }
}

/// Saved as `phone`: the cell and the seconds.
impl TimedTask for Phone {
    const NAME: &'static str = "phone";

    fn new(cell: Point, seconds: f32) -> Phone {
        Phone { at: cell, seconds }
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
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};

    const HERE: Point = Point::new(3, 3);

    /// The look takes its time and tells the body nothing itself: what was on
    /// the phone is the brain's to say once it is over (`sim::news::read`).
    #[test]
    fn a_look_at_the_phone_takes_its_time_and_is_over() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), HERE);
        rig.biology.edit(|stats| stats.with_fun(30.0));
        let mut task = Task::phone(HERE, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
        assert_eq!(rig.biology.stats().fun(), 30.0, "the brain's to say, not the task's");
    }

    #[test]
    fn a_unit_moved_off_its_spot_puts_the_phone_away() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), HERE);
        assert_eq!(rig.tick(&mut Task::phone(Point::new(5, 5), 1.0)), TaskResult::Failed);
    }
}
