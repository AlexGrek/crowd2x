//! [`Sleep`]: get into a bed and sleep in it for a while.
//!
//! The second task built on [`Access::Entered`](crate::sim::feature::Access) —
//! a bed is lain in, not stood next to — so it is [`UseToilet`](super::UseToilet)'s
//! shape: walk into the cell, then interact with it. What the body gets out of
//! it is [`Event::Slept`] when the time is up, and how much rest that is is the
//! energy process's to say.
//!
//! One task is **one stretch of sleep**, an hour of world at the length the
//! goal queues it. A night is several of them, one after another, and that is
//! the design: whoever is in charge between two stretches can be somebody
//! else.

use crate::map::Point;
use crate::sim::biology::Event;
use crate::sim::clock::TIME_SCALE;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sleep {
    /// The bed's own cell — where this task walks the body *into*, not
    /// beside. A goal is expected to have already got the unit within one step
    /// of it (`goals::stand_beside`); this task covers the last one.
    pub bed: Point,
    /// How long this stretch of sleep lasts, in **watched** seconds — the
    /// unit every action is measured in. The body is told it in world seconds
    /// ([`TIME_SCALE`] more), which is what rest is worth.
    pub seconds: f32,
}

impl TaskExecutor for Sleep {
    /// **Two stages, one action slot**, exactly as [`UseToilet`](super::UseToilet)
    /// has them: with nothing running, start walking straight into the bed
    /// ([`Action::enter`]) unless already lying in it, in which case start
    /// sleeping. Which stage just finished is read off the action's own shape.
    ///
    /// **Checked every tick**, getting in or asleep: a kind with no body has
    /// nothing to rest, and a unit pulled more than a step away from the bed
    /// fails the task outright and is no more rested than it was.
    ///
    /// The bed's cell is claimed through
    /// [`Occupancy`](crate::sim::occupancy::Occupancy) the moment somebody is
    /// in it, so a second unit trying at the same moment is blocked the way a
    /// body in a doorway blocks a walk — one sleeper to a bed — and the goal
    /// that queued this decides what that means.
    ///
    /// The body hears [`Event::Slept`] only once the action is over: half an
    /// hour's sleep that was cut short is not a stretch of sleep at all.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.bed) > 1 || ctx.biology.is_none() {
            return TaskResult::Failed;
        }

        if ctx.action.is_none() {
            *ctx.action = if ctx.here() == self.bed {
                Action::interact(self.bed, self.seconds)
            } else {
                Action::enter(ctx.walk, self.bed)
            };
            return TaskResult::Executing;
        }

        let getting_in = matches!(ctx.action, Action::Move { .. });
        match ctx.advance_action() {
            ActionState::Finished if getting_in => {
                // In: the sleep starts on the next tick's own terms, like any
                // other freshly started action.
                *ctx.action = Action::interact(self.bed, self.seconds);
                TaskResult::Executing
            }
            ActionState::Finished => {
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Slept {
                        world_seconds: self.seconds * TIME_SCALE,
                    });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("sleep in the bed at {}, {}", self.bed.x, self.bed.y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::clock::{watched, HOUR};

    fn rig_at(cell: Point, stamina: f32) -> Rig {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), cell);
        rig.biology.edit(|stats| stats.with_stamina(stamina));
        rig
    }

    #[test]
    fn sleeping_in_a_bed_from_across_the_room_fails_without_starting() {
        let mut rig = rig_at(Point::new(1, 1), 20.0);
        let mut task = Task::sleep(Point::new(6, 6), 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.biology.stats().stamina(), 20.0);
    }

    #[test]
    fn a_stretch_of_sleep_is_worth_something_only_once_it_is_over() {
        let mut rig = rig_at(Point::new(5, 6), 20.0);
        let mut task = Task::sleep(Point::new(6, 6), 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.biology.stats().stamina(), 20.0, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert!(rig.biology.stats().stamina() > 20.0, "{}", rig.biology.stats().stamina());
    }

    /// The conversion this task exists to make: it is timed in watched seconds
    /// and rest is worth world ones.
    #[test]
    fn an_hour_of_world_in_bed_is_an_eighth_of_a_night() {
        let mut rig = rig_at(Point::new(5, 6), 20.0);
        let mut task = Task::sleep(Point::new(6, 6), watched(HOUR));
        assert_eq!(rig.run(&mut task, 5000), TaskResult::Success);
        let gained = rig.biology.stats().stamina() - 20.0;
        assert!((gained - 100.0 / 8.0 - 100.0 / 16.0).abs() < 0.5, "gained {gained}");
    }

    #[test]
    fn leaving_the_bed_mid_sleep_fails_it_and_rests_nothing() {
        let mut rig = rig_at(Point::new(5, 6), 20.0);
        let mut task = Task::sleep(Point::new(6, 6), 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);

        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().stamina(), 20.0);
    }

    /// A bed is slept in, so — like the toilet, unlike the computer — the
    /// unit ends up standing in its cell.
    #[test]
    fn sleeping_walks_into_the_beds_own_cell_before_sleeping() {
        let bed = Point::new(6, 6);
        let mut rig = rig_at(Point::new(5, 6), 20.0);
        let mut task = Task::sleep(bed, 0.5);

        let mut lay_down = false;
        for _ in 0..200 {
            let result = rig.tick(&mut task);
            assert_ne!(result, TaskResult::Failed, "should not have been refused");
            if matches!(rig.action, Action::Interact { .. }) {
                lay_down = true;
                break;
            }
        }
        assert!(lay_down, "never lay down");
        assert_eq!(rig.walk.body().center_position(), bed, "should be in it by now");
        assert_eq!(rig.biology.stats().stamina(), 20.0, "not slept yet, only got in");

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.walk.body().center_position(), bed, "still there when it finished");
    }

    #[test]
    fn already_lying_in_the_bed_skips_straight_to_sleeping() {
        let bed = Point::new(6, 6);
        let mut rig = rig_at(bed, 20.0);
        let mut task = Task::sleep(bed, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert!(
            matches!(rig.action, Action::Interact { .. }),
            "already in, so no getting in left to do; action was {:?}",
            rig.action
        );
    }

    /// What `game::props` reads to say a bed is in use, once a bed has art
    /// that can show it.
    #[test]
    fn a_sleeper_is_interacting_with_the_bed_it_is_in() {
        let bed = Point::new(6, 6);
        let mut rig = rig_at(bed, 20.0);
        let mut task = Task::sleep(bed, 5.0);
        let _ = rig.tick(&mut task);
        assert_eq!(rig.action.interacting_with(), Some(bed));
    }
}
