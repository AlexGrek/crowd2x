//! [`UseComputer`]: sit at a computer and have a go on it.
//!
//! The first task that is only about time passing next to a prop — nothing
//! changes hands, nothing is carried away, and what the body gets out of it
//! is [`Event::Entertained`] when the session is over.

use crate::map::Point;
use crate::sim::biology::Event;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct UseComputer {
    /// The computer's cell. A computer is used from **beside** it
    /// ([`Access::Beside`](crate::sim::feature::Access::Beside)) — its own
    /// cell is furniture and nobody stands in it — so this is what the unit
    /// is turned towards, never where it walks to.
    pub computer: Point,
    /// How long a go on it takes.
    pub seconds: f32,
}

impl TaskExecutor for UseComputer {
    /// **Only from one step away, checked every tick**, like
    /// [`TakeItem`](super::TakeItem): this task does not walk, and a unit
    /// pulled away from the desk part way through has not had its go. A kind
    /// with no body has nothing that could be entertained either.
    ///
    /// The body hears [`Event::Entertained`] only once the action is over —
    /// half a go is no fun at all. A go had again while the last one is still
    /// in recent memory is less fun and less of a treat, and the log says so.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.computer) > 1 || ctx.biology.is_none() {
            return TaskResult::Failed;
        }

        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.computer, self.seconds);
            return TaskResult::Executing;
        }

        match ctx.advance_action() {
            ActionState::Finished => {
                let recalled = ctx.biology.as_deref_mut().and_then(|b| b.handle(Event::Entertained));
                super::report_recalled(ctx, recalled);
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("use the computer at {}, {}", self.computer.x, self.computer.y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};

    fn rig_at(cell: Point, fun: f32) -> Rig {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), cell);
        rig.biology.edit(|stats| stats.with_fun(fun));
        rig
    }

    #[test]
    fn using_a_computer_from_across_the_room_fails_without_starting() {
        let mut rig = rig_at(Point::new(1, 1), 20.0);
        let mut task = Task::use_computer(Point::new(6, 6), 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }

    #[test]
    fn a_go_on_it_is_worth_something_only_once_it_is_over() {
        let mut rig = rig_at(Point::new(5, 6), 20.0);
        let mut task = Task::use_computer(Point::new(6, 6), 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.biology.stats().fun(), 20.0, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert!(rig.biology.stats().fun() > 20.0, "{}", rig.biology.stats().fun());
    }

    #[test]
    fn walking_away_mid_session_fails_it_and_is_no_fun_at_all() {
        let mut rig = rig_at(Point::new(5, 6), 20.0);
        let mut task = Task::use_computer(Point::new(6, 6), 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);

        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }

    #[test]
    fn a_second_go_the_same_day_is_less_fun_and_in_recent_memory() {
        let mut rig = rig_at(Point::new(5, 6), 0.0);
        rig.biology.edit(|stats| stats.with_satisfaction(0.0));
        let mut first = Task::use_computer(Point::new(6, 6), 0.5);
        assert_eq!(rig.run(&mut first, 600), TaskResult::Success);
        let after_first = *rig.biology.stats();
        assert!(!rig.world.log.drain().iter().any(|l| l.contains("in recent memory")));

        let mut second = Task::use_computer(Point::new(6, 6), 0.5);
        assert_eq!(rig.run(&mut second, 600), TaskResult::Success);
        let after_second = *rig.biology.stats();

        let fun = (after_first.fun(), after_second.fun() - after_first.fun());
        assert!(fun.1 > 0.0 && fun.1 < fun.0 * 0.6, "the second go is about half the fun: {fun:?}");
        let treat = (after_first.satisfaction(), after_second.satisfaction() - after_first.satisfaction());
        assert!(treat.1 > 0.0 && treat.1 < treat.0 * 0.6, "and half the treat: {treat:?}");
        let lines = rig.world.log.drain();
        assert!(lines.iter().any(|l| l.contains("had entertainment in recent memory")), "{lines:?}");
    }

    /// A computer is used from beside it, so the unit stays where it is
    /// throughout — unlike the toilet, which is used by walking into it.
    #[test]
    fn using_a_computer_never_moves_anybody_onto_it() {
        let computer = Point::new(6, 6);
        let beside = Point::new(5, 6);
        let mut rig = rig_at(beside, 20.0);
        let mut task = Task::use_computer(computer, 0.5);

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.walk.body().center_position(), beside);
    }
}
