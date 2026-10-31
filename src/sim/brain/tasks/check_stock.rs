//! [`CheckStock`]: a shopkeeper looks in the store's superfridge, and **calls
//! the courier dispatch when it is at its call level or less**.
//!
//! From beside the superfridge, a minute's look. What it sees is the world's
//! ([`Think::superfridge`](crate::sim::Think), [`Think::bins`](crate::sim::Think)):
//! how full it is, which it says in the log, and — at
//! [`StoreRules::call_at`](crate::sim::superfridge::StoreRules) percent or less
//! — the call, asked of the world ([`Effect::CallDispatch`]), which keeps one
//! waiting call a store ([`crate::sim::dispatch::Dispatch::call`]).
//!
//! It is work, and tires like it ([`Event::Worked`]).

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::feature::FeatureKind;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// A minute of world to look over the superfridge.
pub const CHECK_SECONDS: f32 = watched(MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CheckStock {
    /// The superfridge's cell.
    pub fridge: Point,
    /// How long the look takes, in **watched** seconds.
    pub seconds: f32,
}

impl TaskExecutor for CheckStock {
    /// **Checked every tick**: one step from a superfridge, or it fails.
    /// Finishing says how full it is, and calls the dispatch if that is
    /// little enough.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.fridge) > 1 || !ctx.think.features.has(FeatureKind::Superfridge, self.fridge) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.fridge, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let (superfridge, bins) = (ctx.think.superfridge, ctx.think.bins);
                let uid = ctx.walk.body().uid();
                let percent = superfridge.percent_at(self.fridge, bins);
                if superfridge.wants_food_at(self.fridge, bins) {
                    *ctx.effect = Effect::CallDispatch { at: self.fridge };
                } else {
                    ctx.think.log.unit(uid, format!(
                        "{uid} checked the superfridge at {}, {}: {percent:.0}% full",
                        self.fridge.x, self.fridge.y
                    ));
                }
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
        format!("check the superfridge at {}, {}", self.fridge.x, self.fridge.y)
    }
}

impl Task {
    /// Look over the superfridge in `fridge`, from a cell beside it.
    pub const fn check_stock(fridge: Point, seconds: f32) -> Task {
        Task::CheckStock(CheckStock { fridge, seconds })
    }
}

/// Saved as `check stock`: the superfridge's cell and the seconds.
impl TimedTask for CheckStock {
    const NAME: &'static str = "check stock";

    fn new(cell: Point, seconds: f32) -> CheckStock {
        CheckStock { fridge: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.fridge
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

    const FRIDGE: Point = Point::new(6, 6);

    /// A shopkeeper in `cell` of a room with a superfridge holding `stored`
    /// of its 100.
    fn rig_at(cell: Point, stored: u32) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "superfridge", FRIDGE);
        let mut rig = Rig::new(map, cell);
        assert!(rig.world.bins.set(FRIDGE, stored));
        rig
    }

    fn rig(stored: u32) -> Rig {
        rig_at(Point::new(5, 6), stored)
    }

    /// `store.yaml`'s call level, in whole batches of the 100.
    fn call_level() -> u32 {
        crate::sim::superfridge::StoreRules::default().call_at as u32
    }

    #[test]
    fn a_superfridge_at_the_call_level_or_under_calls_the_dispatch() {
        let mut low = rig(call_level());
        let mut task = Task::check_stock(FRIDGE, 0.5);
        assert_eq!(low.run(&mut task, 600), TaskResult::Success);
        assert_eq!(low.effect, Effect::CallDispatch { at: FRIDGE }, "{}% is the call level", call_level());
    }

    #[test]
    fn a_superfridge_with_food_enough_is_only_looked_at() {
        let mut full = rig(call_level() + 1);
        let mut task = Task::check_stock(FRIDGE, 0.5);
        assert_eq!(full.run(&mut task, 600), TaskResult::Success);
        assert_eq!(full.effect, Effect::None);
        let said = format!("checked the superfridge at 6, 6: {}% full", call_level() + 1);
        assert!(full.world.log.drain().iter().any(|line| line.contains(&said)));
    }

    #[test]
    fn it_is_looked_over_from_beside_it() {
        let mut far = rig_at(Point::new(1, 1), 10);
        assert_eq!(far.tick(&mut Task::check_stock(FRIDGE, 0.5)), TaskResult::Failed);
    }
}
