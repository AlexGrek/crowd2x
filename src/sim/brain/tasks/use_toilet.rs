//! [`UseToilet`]: walk into the toilet and use it — the one feature used by
//! entering rather than touching (see [`crate::sim::feature::Access`]).

use crate::map::Point;
use crate::sim::biology::Event;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct UseToilet {
    /// The toilet's own cell — where this task walks the body *into*, not
    /// beside. A goal is expected to have already got the unit within one
    /// step of it (`goals::stand_beside`); this task covers the last one.
    pub toilet: Point,
    /// How long using it takes, once inside.
    pub seconds: f32,
}

impl TaskExecutor for UseToilet {
    /// **Two stages, one action slot.** With nothing running, start walking
    /// straight into `self.toilet` ([`Action::enter`]) unless already
    /// standing in it, in which case start interacting. Which stage just
    /// finished is read off `ctx.action`'s own shape — a `Move` here is
    /// always the entry, since nothing else in this task ever starts one —
    /// rather than kept as separate state.
    ///
    /// **Checked every tick, entering or inside**, like
    /// [`TakeItem`](super::TakeItem): a kind with no body has nothing to
    /// relieve, and a unit pulled more than a step away — before it got in,
    /// or after — fails the task outright.
    ///
    /// Entering can fail like any other move: the toilet's cell is claimed
    /// by [`crate::sim::occupancy::Occupancy`] the moment somebody is
    /// standing in it, so a second unit trying at the same moment is simply
    /// blocked, the way a body in a doorway blocks a walk, and the goal that
    /// queued this decides what that means.
    ///
    /// The body hears [`Event::Relieved`] only once the action is over — a
    /// unit pulled away mid-use is exactly as full as it was.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.toilet) > 1 || ctx.biology.is_none() {
            return TaskResult::Failed;
        }

        if ctx.action.is_none() {
            *ctx.action = if ctx.here() == self.toilet {
                Action::interact(self.toilet, self.seconds)
            } else {
                Action::enter(ctx.walk, self.toilet)
            };
            return TaskResult::Executing;
        }

        let entering = matches!(ctx.action, Action::Move { .. });
        match ctx.advance_action() {
            ActionState::Finished if entering => {
                // In the door: the sit-down starts on the next tick's own
                // terms, same as any other freshly started action.
                *ctx.action = Action::interact(self.toilet, self.seconds);
                TaskResult::Executing
            }
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

    #[test]
    fn using_the_toilet_walks_into_its_own_cell_before_interacting() {
        // The one thing this task is for: standing beside it is not enough,
        // and finishing means having actually walked onto its cell — not a
        // fiction, a real change of position, checked while it is still
        // under way.
        let toilet = Point::new(6, 6);
        let mut rig = rig_at(Point::new(5, 6));
        let mut task = Task::use_toilet(toilet, 0.5);

        // `center_position` crosses into the toilet's cell well before the
        // walk is centred and the entering `Move` reports itself finished
        // (`Walker::advance`'s `is_centred` check) — so what marks "done
        // entering, now interacting" is the action itself, not the cell.
        let mut sat_down = false;
        for _ in 0..200 {
            let result = rig.tick(&mut task);
            assert_ne!(result, TaskResult::Failed, "should not have been refused entry");
            if matches!(rig.action, Action::Interact { .. }) {
                sat_down = true;
                break;
            }
        }
        assert!(sat_down, "never sat down to use it");
        assert_eq!(rig.walk.body().center_position(), toilet, "should be standing inside it by now");
        assert_eq!(rig.biology.stats().bladder(), 90.0, "not used yet, only entered");

        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.biology.stats().bladder(), 0.0);
        assert_eq!(rig.walk.body().center_position(), toilet, "still there when it finished");
    }

    #[test]
    fn already_standing_in_the_toilet_skips_straight_to_using_it() {
        let toilet = Point::new(6, 6);
        let mut rig = rig_at(toilet);
        let mut task = Task::use_toilet(toilet, 0.5);

        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert!(
            matches!(rig.action, Action::Interact { .. }),
            "already inside, so no entering left to do; action was {:?}",
            rig.action
        );
    }
}
