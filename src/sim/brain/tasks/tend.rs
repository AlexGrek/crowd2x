//! [`Tend`]: tend a growing crop — weed it, water it — once a day.
//!
//! What a farmer does with most of its shift (`FarmGoal`): from beside the
//! crop, a quarter of an hour's work, and the world is *asked* to count it
//! ([`Effect::Tend`]), which raises the crop's quality for the day
//! (`Fields::tend`). A crop tended already today, or ripe, wants nothing.
//!
//! It is work, and tires like it ([`Event::Worked`]).

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// A quarter of an hour of world to tend a crop.
pub const TEND_SECONDS: f32 = watched(15.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tend {
    /// The crop's cell.
    pub crop: Point,
    /// How long tending it takes, in **watched** seconds.
    pub seconds: f32,
}

impl Tend {
    /// Whether it can go on: beside the crop, and the crop still wanting
    /// tending today.
    fn can_tend(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.here().manhattan_distance(self.crop) == 1
            && ctx.think.fields.at(self.crop).is_some_and(|crop| crop.needs_tending(ctx.think.clock))
    }
}

impl TaskExecutor for Tend {
    /// **Checked every tick**: a unit pulled away, or a crop that ripened or
    /// went, ends the work, and nothing is asked for.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_tend(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.crop, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                ctx.walk.body_mut().dirt_mut().soil(crate::sim::dirt::TENDING);
                *ctx.effect = Effect::Tend { at: self.crop, when: ctx.think.clock };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before it was done: the crop is no better for it, but the
    /// time spent was still work.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("tend the wheat at {}, {}", self.crop.x, self.crop.y)
    }
}

impl Task {
    /// Tend the crop in `crop`, from beside it.
    pub const fn tend(crop: Point, seconds: f32) -> Task {
        Task::Tend(Tend { crop, seconds })
    }
}

/// Saved as `tend`: the crop's cell and the seconds.
impl TimedTask for Tend {
    const NAME: &'static str = "tend";

    fn new(cell: Point, seconds: f32) -> Tend {
        Tend { crop: cell, seconds }
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
    fn a_crop_is_tended_the_day_after_it_was_sown_and_the_world_asked_to_count_it() {
        let mut rig = rig_on(2);
        let mut task = Task::tend(CROP, TEND_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::Dirt::CLEAN);
        assert_eq!(rig.run(&mut task, 10_000), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Tend { at: CROP, when: rig.world.clock });
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::TENDING, "weeding is dirty work");
    }

    #[test]
    fn not_the_day_it_was_sown_not_once_ripe_and_not_where_nothing_grows() {
        assert_eq!(rig_on(1).tick(&mut Task::tend(CROP, TEND_SECONDS)), TaskResult::Failed, "sown today");
        assert_eq!(rig_on(8).tick(&mut Task::tend(CROP, TEND_SECONDS)), TaskResult::Failed, "ripe");
        assert_eq!(rig_on(2).tick(&mut Task::tend(Point::new(2, 2), TEND_SECONDS)), TaskResult::Failed, "nothing there");
    }
}
