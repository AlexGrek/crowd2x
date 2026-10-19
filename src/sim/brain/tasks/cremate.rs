//! [`Cremate`]: put the body in hand into a crematorium.
//!
//! The end of an undertaker's call (`BuryGoal`): from beside its crematorium,
//! a quarter of an hour's work, and the body is gone. The hand is the unit's
//! own and is emptied here, when the work finishes; the world is told
//! ([`Effect::Cremate`]) for the log.
//!
//! It is work, and tires like it ([`Event::Worked`]).

use crate::map::Point;
use crate::save::pb;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// A quarter of an hour of world to see a body into the fire.
pub const CREMATE_SECONDS: f32 = watched(15.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cremate {
    /// The crematorium's cell: what the unit is turned towards.
    pub crematorium: Point,
    /// How long it takes, in **watched** seconds.
    pub seconds: f32,
}

impl Cremate {
    /// Whether it can go on: one step from the crematorium, the crematorium
    /// still there, and a body in hand.
    fn can_cremate(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.here().manhattan_distance(self.crematorium) <= 1
            && ctx.think.features.has(FeatureKind::Crematorium, self.crematorium)
            && ctx.inventory.as_deref().is_some_and(|inventory| inventory.hand() == Some(ItemKind::Body))
    }
}

impl TaskExecutor for Cremate {
    /// **Checked every tick**: pulled away, or the crematorium gone, and the
    /// body is still in hand. Finishing empties the hand, tells the world and
    /// tells the body it worked.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_cremate(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.crematorium, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let Some(inventory) = ctx.inventory.as_deref_mut() else {
                    return TaskResult::Failed;
                };
                let _ = inventory.set_hand(None);
                *ctx.effect = Effect::Cremate { at: self.crematorium };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
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
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("cremate a body at the crematorium at {}, {}", self.crematorium.x, self.crematorium.y)
    }
}

impl Task {
    /// Put the body in hand into the crematorium in `crematorium`, from a
    /// cell beside it.
    pub const fn cremate(crematorium: Point, seconds: f32) -> Task {
        Task::Cremate(Cremate { crematorium, seconds })
    }
}

/// Saved as `cremate`: the crematorium's cell and the seconds.
impl TimedTask for Cremate {
    const NAME: &'static str = "cremate";

    fn new(cell: Point, seconds: f32) -> Cremate {
        Cremate { crematorium: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.crematorium
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }

    fn legacy(saved: &pb::TaskKind) -> Option<&pb::Timed> {
        match saved {
            pb::TaskKind::Cremate(task) => Some(task),
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
    use crate::sim::testing::prop_at;

    const CREMATORIUM: Point = Point::new(4, 3);

    fn rig() -> Rig {
        let mut map = Map::new(Size::new(8, 6), FLOOR);
        prop_at(&mut map, "crematorium", CREMATORIUM);
        Rig::new(map, Point::new(3, 3))
    }

    #[test]
    fn the_body_goes_once_the_work_is_over() {
        let mut rig = rig();
        rig.set_hand(Some(ItemKind::Body));
        let mut task = Task::cremate(CREMATORIUM, CREMATE_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.hand(), Some(ItemKind::Body), "not yet");
        assert_eq!(rig.run(&mut task, 10_000), TaskResult::Success);
        assert_eq!(rig.hand(), None);
        assert_eq!(rig.effect, Effect::Cremate { at: CREMATORIUM });
    }

    #[test]
    fn with_nothing_in_hand_or_away_from_it_it_fails() {
        let mut rig = rig();
        assert_eq!(rig.tick(&mut Task::cremate(CREMATORIUM, CREMATE_SECONDS)), TaskResult::Failed);
        rig.set_hand(Some(ItemKind::Body));
        assert_eq!(rig.tick(&mut Task::cremate(Point::new(7, 5), CREMATE_SECONDS)), TaskResult::Failed);
        assert_eq!(rig.hand(), Some(ItemKind::Body));
    }
}
