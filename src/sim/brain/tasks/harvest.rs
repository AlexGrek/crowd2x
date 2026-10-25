//! [`Harvest`]: cut a ripe crop and take the wheat.
//!
//! The end of a crop (`FarmGoal`): from beside it, ten minutes' work, with
//! room in the pack for all it yields ([`Crop::harvest`]) — the crop is the
//! world's, so it is *asked* for ([`Effect::Harvest`]), and the world step
//! takes it out of the fields, opens its plot and puts the wheat in the pack
//! (`effect::apply`).
//!
//! It is work, and tires like it ([`Event::Worked`]).
//!
//! [`Crop::harvest`]: crate::sim::fields::Crop::harvest

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::item::ItemKind;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Ten minutes of world to harvest a crop.
pub const HARVEST_SECONDS: f32 = watched(10.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Harvest {
    /// The crop's cell.
    pub crop: Point,
    /// How long harvesting it takes, in **watched** seconds.
    pub seconds: f32,
}

impl Harvest {
    /// Whether it can go on: beside the crop, the crop ripe, and room in the
    /// pack for every sheaf it yields.
    fn can_harvest(&self, ctx: &TaskCtx<'_>) -> bool {
        let Some(crop) = ctx.think.fields.at(self.crop) else {
            return false;
        };
        ctx.here().manhattan_distance(self.crop) == 1
            && crop.is_ripe(ctx.think.clock)
            && ctx.inventory.as_deref().is_some_and(|pack| pack.room_for_many(ItemKind::Wheat, crop.harvest()))
    }
}

impl TaskExecutor for Harvest {
    /// **Checked every tick**: a unit pulled away, a crop gone or a pack that
    /// filled ends the work, and nothing is asked for.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_harvest(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.crop, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                ctx.walk.body_mut().dirt_mut().soil(crate::sim::dirt::HARVESTING);
                *ctx.effect = Effect::Harvest { at: self.crop, when: ctx.think.clock };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before it was cut: the time spent was still work.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("harvest the wheat at {}, {}", self.crop.x, self.crop.y)
    }
}

impl Task {
    /// Harvest the ripe crop in `crop`, from beside it.
    pub const fn harvest(crop: Point, seconds: f32) -> Task {
        Task::Harvest(Harvest { crop, seconds })
    }
}

/// Saved as `harvest`: the crop's cell and the seconds.
impl TimedTask for Harvest {
    const NAME: &'static str = "harvest";

    fn new(cell: Point, seconds: f32) -> Harvest {
        Harvest { crop: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.crop
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
    use crate::map::{Map, Size, MEADOW};
    use crate::sim::clock::{Clock, DAY};

    const CROP: Point = Point::new(4, 4);

    /// Beside a crop sown at opening time on the Monday, on `day`.
    fn rig_on(day: u32) -> Rig {
        let mut rig = Rig::new(Map::new(Size::new(8, 8), MEADOW), Point::new(4, 3));
        let sown = rig.world.clock;
        let crate::sim::testing::World { map, facilities, .. } = &mut rig.world;
        assert!(facilities.fields.sow(map, CROP, Point::new(0, 1), sown));
        rig.world.clock = Clock::after_watching((day - 1) as f64 * DAY as f64 / crate::sim::clock::TIME_SCALE as f64);
        rig
    }

    #[test]
    fn a_ripe_crop_is_asked_for_once_the_work_is_over() {
        let mut rig = rig_on(4);
        let mut task = Task::harvest(CROP, HARVEST_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::Dirt::CLEAN);
        assert_eq!(rig.run(&mut task, 10_000), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Harvest { at: CROP, when: rig.world.clock });
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::HARVESTING);
        assert_eq!(rig.inventory.count(ItemKind::Wheat), 0, "the world puts it in the pack, not the task");
    }

    #[test]
    fn not_before_it_is_ripe_and_not_into_a_full_pack() {
        assert_eq!(rig_on(3).tick(&mut Task::harvest(CROP, HARVEST_SECONDS)), TaskResult::Failed, "in ear");
        let mut full = rig_on(4);
        while full.inventory.stow(ItemKind::Wheat) {}
        assert_eq!(full.tick(&mut Task::harvest(CROP, HARVEST_SECONDS)), TaskResult::Failed, "no room");
    }
}
