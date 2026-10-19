//! [`TakeBody`]: take up the body lying in a cell.
//!
//! The first half of an undertaker's call (`BuryGoal`): from beside the body,
//! or standing over it, a few minutes' work. The body is the world's until
//! it is taken, so it is *asked* for ([`Effect::TakeBody`]) and the world step
//! puts it in the hand (`sim::take_body`) — a second undertaker at the same
//! body on the same tick gets nothing, and the body leaves the world at the
//! end of the tick.
//!
//! It is work, and tires like it ([`Event::Worked`]).

use crate::map::Point;
use crate::save::pb;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Five minutes of world to take up a body.
pub const TAKE_BODY_SECONDS: f32 = watched(5.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TakeBody {
    /// The cell the body lies in.
    pub body: Point,
    /// How long taking it up takes, in **watched** seconds.
    pub seconds: f32,
}

impl TakeBody {
    /// Whether it can go on: within a step of the body, the body still
    /// there, and a hand free to take it.
    fn can_take(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.here().manhattan_distance(self.body) <= 1
            && ctx.think.corpses.at(self.body).is_some()
            && ctx.inventory.as_deref().is_some_and(|inventory| inventory.hand().is_none())
    }
}

impl TaskExecutor for TakeBody {
    /// **Checked every tick**: a unit pulled away, a body taken by somebody
    /// else or a hand that filled ends the work, and nothing is asked for.
    /// Finishing asks the world for the body and tells the body it worked.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_take(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.body, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::TakeBody { at: self.body };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before the body was up: the time spent was still work.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("take up the body at {}, {}", self.body.x, self.body.y)
    }
}

impl Task {
    /// Take up the body lying in `body`, from beside it or over it.
    pub const fn take_body(body: Point, seconds: f32) -> Task {
        Task::TakeBody(TakeBody { body, seconds })
    }
}

/// Saved as `take body`: the body's cell and the seconds.
impl TimedTask for TakeBody {
    const NAME: &'static str = "take body";

    fn new(cell: Point, seconds: f32) -> TakeBody {
        TakeBody { body: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.body
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }

    fn legacy(saved: &pb::TaskKind) -> Option<&pb::Timed> {
        match saved {
            pb::TaskKind::TakeBody(task) => Some(task),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::item::ItemKind;
    use crate::sim::uid::{EntityType, Uid};

    fn rig_beside_a_body() -> (Rig, Point) {
        let body = Point::new(4, 3);
        let mut rig = Rig::new(Map::new(Size::new(8, 6), FLOOR), Point::new(3, 3));
        rig.world.corpses.add(Uid::new(EntityType::Human, 1), body);
        (rig, body)
    }

    #[test]
    fn a_body_is_asked_for_once_the_work_is_over_and_not_before() {
        let (mut rig, body) = rig_beside_a_body();
        let mut task = Task::take_body(body, TAKE_BODY_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        let tired = rig.biology.stats().stamina();
        assert_eq!(rig.run(&mut task, 10_000), TaskResult::Success);
        assert_eq!(rig.effect, Effect::TakeBody { at: body });
        assert_eq!(rig.hand(), None, "the world puts it in the hand, not the task");
        assert!(rig.biology.stats().stamina() < tired, "it is work");
    }

    #[test]
    fn from_across_the_room_with_a_full_hand_or_with_no_body_it_fails() {
        let (mut rig, body) = rig_beside_a_body();
        let mut far = Task::take_body(Point::new(7, 5), TAKE_BODY_SECONDS);
        assert_eq!(rig.tick(&mut far), TaskResult::Failed);
        rig.set_hand(Some(ItemKind::Food));
        assert_eq!(rig.tick(&mut Task::take_body(body, TAKE_BODY_SECONDS)), TaskResult::Failed);
        rig.set_hand(None);
        rig.world.corpses = Default::default();
        assert_eq!(rig.tick(&mut Task::take_body(body, TAKE_BODY_SECONDS)), TaskResult::Failed);
    }
}
