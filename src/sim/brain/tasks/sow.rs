//! [`Sow`]: sow a crop of wheat in a plot of good soil.
//!
//! A farmer's field is begun and kept full a crop at a time (`FarmGoal`):
//! from beside the plot, ten minutes' work, and the crop is *asked* for
//! ([`Effect::Sow`]) — the field is the world's, so the world step sows it
//! (`effect::apply`), for the farm the farmer works for, and refuses where
//! somebody is standing in the plot or somebody else sowed it first.
//!
//! It is work, and tires like it ([`Event::Worked`]).

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Ten minutes of world to sow a plot.
pub const SOW_SECONDS: f32 = watched(10.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sow {
    /// The plot: what the unit is turned towards, never where it stands.
    pub plot: Point,
    /// How long sowing takes, in **watched** seconds.
    pub seconds: f32,
}

impl Sow {
    /// Whether it can go on: beside the plot, and the plot still free good
    /// soil.
    fn can_sow(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.here().manhattan_distance(self.plot) == 1 && ctx.think.map.can_sow(self.plot)
    }
}

impl TaskExecutor for Sow {
    /// **Checked every tick**: a unit pulled away or a plot sown by somebody
    /// else ends the work, and nothing is asked for. Finishing asks the world
    /// for the crop and tells the body it worked.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_sow(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.plot, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                ctx.walk.body_mut().dirt_mut().soil(crate::sim::dirt::SOWING);
                *ctx.effect = Effect::Sow { at: self.plot, when: ctx.think.clock };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before the seed was in: the time spent was still work.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("sow wheat at {}, {}", self.plot.x, self.plot.y)
    }
}

impl Task {
    /// Sow a crop in `plot`, from beside it.
    pub const fn sow(plot: Point, seconds: f32) -> Task {
        Task::Sow(Sow { plot, seconds })
    }
}

/// Saved as `sow`: the plot and the seconds.
impl TimedTask for Sow {
    const NAME: &'static str = "sow";

    fn new(cell: Point, seconds: f32) -> Sow {
        Sow { plot: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.plot
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

    const PLOT: Point = Point::new(4, 4);

    #[test]
    fn a_crop_is_asked_for_once_the_work_is_over_and_not_before() {
        let mut rig = Rig::new(Map::new(Size::new(8, 8), MEADOW), Point::new(3, 4));
        rig.biology.edit(|stats| stats.with_stamina(80.0).with_fun(40.0));
        let mut task = Task::sow(PLOT, SOW_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.action.interacting_with(), Some(PLOT));
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::Dirt::CLEAN, "no grime before the seed is in");
        assert_eq!(rig.run(&mut task, 10_000), TaskResult::Success);
        assert!(matches!(rig.effect, Effect::Sow { at: PLOT, .. }), "{:?}", rig.effect);
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::SOWING, "hands in the soil");
        assert!(rig.biology.stats().stamina() < 80.0, "it is work");
        assert_eq!(rig.biology.stats().fun(), 40.0, "and no fun");
    }

    #[test]
    fn not_from_across_the_field_not_on_the_plot_and_not_where_nothing_can_grow() {
        let mut rig = Rig::new(Map::new(Size::new(8, 8), MEADOW), Point::new(1, 1));
        assert_eq!(rig.tick(&mut Task::sow(PLOT, SOW_SECONDS)), TaskResult::Failed, "too far");
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::Dirt::CLEAN, "nothing sown, nothing soiled");
        assert_eq!(rig.tick(&mut Task::sow(Point::new(1, 2), SOW_SECONDS)), TaskResult::Failed, "not a plot");
        let mut on = Rig::new(Map::new(Size::new(8, 8), MEADOW), PLOT);
        assert_eq!(on.tick(&mut Task::sow(PLOT, SOW_SECONDS)), TaskResult::Failed, "standing on it");
        let mut floor = Rig::new(Map::new(Size::new(8, 8), crate::map::FLOOR), Point::new(3, 4));
        assert_eq!(floor.tick(&mut Task::sow(PLOT, SOW_SECONDS)), TaskResult::Failed, "no soil");
        let mut sown = Rig::new(Map::new(Size::new(8, 8), MEADOW), Point::new(3, 4));
        assert!(sown.world.map.plant(PLOT));
        assert_eq!(sown.tick(&mut Task::sow(PLOT, SOW_SECONDS)), TaskResult::Failed, "already sown");
    }
}
