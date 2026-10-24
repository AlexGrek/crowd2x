//! Cook one stored meal from wheat in the pack, beside a powered cooker.
//! Half an hour of world time is work; interruption preserves the wheat
//! and reports the time already worked to the body's energy process.

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::feature::FeatureKind;
use crate::sim::item::{ItemKind, WHEAT_PER_MEAL};

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Half an hour of world at the cooker for a meal.
pub const COOK_SECONDS: f32 = watched(30.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cook {
    /// The cooker's cell: what the unit is turned towards, never where it
    /// stands.
    pub cooker: Point,
    /// How long a meal takes, in **watched** seconds.
    pub seconds: f32,
}

impl Cook {
    /// Whether it can go on: one step from the cooker, the cooker working
    /// (it needs power), and enough wheat in the pack for a meal.
    fn can_cook(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.here().manhattan_distance(self.cooker) <= 1
            && ctx.think.features.has(FeatureKind::FoodCooker, self.cooker)
            && ctx.inventory.as_deref().is_some_and(|inventory| inventory.count(ItemKind::Wheat) >= WHEAT_PER_MEAL)
    }
}

impl TaskExecutor for Cook {
    /// **Checked every tick**: a unit pulled away, a cooker switched off or
    /// a pack without the wheat ends the work there, and nothing is made.
    /// Finishing takes the wheat out of the pack and puts the meal in.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_cook(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.cooker, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let Some(inventory) = ctx.inventory.as_deref_mut() else {
                    return TaskResult::Failed;
                };
                for _ in 0..WHEAT_PER_MEAL {
                    let _ = inventory.take_out(ItemKind::Wheat);
                }
                // Wheat and meals have the same mass and volume, so there is
                // always room for it.
                let _ = inventory.stow(ItemKind::Meal);
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

    /// Pulled away before the meal was made: nothing came of it, but the
    /// time spent at it was still work.
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
        format!("cook meals at the cooker at {}, {}", self.cooker.x, self.cooker.y)
    }
}

impl Task {
    /// Cook wheat in the pack into a meal at the cooker in
    /// `cooker`, from a cell beside it.
    pub const fn cook(cooker: Point, seconds: f32) -> Task {
        Task::Cook(Cook { cooker, seconds })
    }
}

/// Saved as `cook`: the cooker's cell and the seconds.
impl TimedTask for Cook {
    const NAME: &'static str = "cook";

    fn new(cell: Point, seconds: f32) -> Cook {
        Cook { cooker: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.cooker
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

    const COOKER: Point = Point::new(6, 6);

    /// A unit in `cell` of a room with a working cooker at [`COOKER`],
    /// carrying `pieces` of wheat.
    fn rig_at(cell: Point, pieces: u8) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "food cooker", COOKER);
        let mut rig = Rig::new(map, cell);
        rig.biology.edit(|stats| stats.with_stamina(80.0).with_fun(40.0));
        for _ in 0..pieces {
            assert!(rig.inventory.stow(ItemKind::Wheat));
        }
        rig
    }

    /// One sheaf of wheat goes in and a meal comes out, only once the work
    /// is over; it tires, and it is no fun; the cooker is what is worked.
    #[test]
    fn wheat_becomes_a_meal_once_the_work_is_over() {
        let mut rig = rig_at(Point::new(5, 6), 3);
        let mut task = Task::cook(COOKER, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.action.interacting_with(), Some(COOKER), "the cooker is being worked");
        assert_eq!(rig.inventory.count(ItemKind::Meal), 0, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!((rig.inventory.count(ItemKind::Wheat), rig.inventory.count(ItemKind::Meal)), (2, 1));
        assert!(rig.biology.stats().stamina() < 80.0, "cooking is work");
        assert_eq!(rig.biology.stats().fun(), 40.0, "and no fun");
    }

    #[test]
    fn no_wheat_means_no_meal() {
        let mut rig = rig_at(Point::new(5, 6), 0);
        assert_eq!(rig.tick(&mut Task::cook(COOKER, 0.5)), TaskResult::Failed);
        assert!(rig.action.is_none());
    }

    #[test]
    fn a_cooker_out_of_reach_or_without_power_cooks_nothing() {
        let mut rig = rig_at(Point::new(1, 1), 2);
        assert_eq!(rig.tick(&mut Task::cook(COOKER, 0.5)), TaskResult::Failed);

        let mut rig = rig_at(Point::new(5, 6), 2);
        let mut task = Task::cook(COOKER, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let dead = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.features = crate::sim::feature::Features::from_map(&rig.world.map, &dead);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.inventory.count(ItemKind::Wheat), 2, "nothing went in");
    }
}
