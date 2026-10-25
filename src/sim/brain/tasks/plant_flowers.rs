//! Plant one flower bed from beside it, with the same work and fatigue as sowing wheat.

use crate::map::Point;
use crate::sim::Effect;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{MINUTE, TIME_SCALE, watched};

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Ten minutes of world to sow a plot.
pub const PLANT_SECONDS: f32 = watched(10.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PlantFlowers {
    /// The plot: what the unit is turned towards, never where it stands.
    pub plot: Point,
    /// How long sowing takes, in **watched** seconds.
    pub seconds: f32,
}

impl PlantFlowers {
    /// Whether it can go on: beside the plot, and the plot still free good
    /// soil.
    fn can_plant(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.here().manhattan_distance(self.plot) == 1
            && crate::sim::flowers::green_plot(ctx.think.map, self.plot)
            && ctx.think.flowers.at(self.plot).is_none()
            && ctx.think.properties.of(self.plot).is_none()
            && !ctx.think.occupancy.is_occupied(self.plot)
            && ctx.think.corpses.at(self.plot).is_none()
            && ctx.biology.is_some()
    }
}

impl TaskExecutor for PlantFlowers {
    /// **Checked every tick**: a unit pulled away or a plot sown by somebody
    /// else ends the work, and nothing is asked for. Finishing asks the world
    /// for the crop and tells the body it worked.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_plant(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.plot, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                ctx.walk.body_mut().dirt_mut().soil(crate::sim::dirt::PLANTING);
                use rand::RngExt;
                let variety = crate::sim::rng::tick_rng(ctx.walk.body().uid(), ctx.think.tick)
                    .random_range(0..crate::sim::flowers::VARIETIES)
                    as u8;
                *ctx.effect = Effect::PlantFlowers {
                    at: self.plot,
                    when: ctx.think.clock,
                    variety,
                };
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

    /// Pulled away before the seed was in: the time spent was still work.
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
        format!("plant flowers at {}, {}", self.plot.x, self.plot.y)
    }
}

impl Task {
    /// Plant a flower bed in `plot`, from beside it.
    pub const fn plant_flowers(plot: Point, seconds: f32) -> Task {
        Task::PlantFlowers(PlantFlowers { plot, seconds })
    }
}

/// Saved as `plant flowers`: the plot and the seconds.
impl TimedTask for PlantFlowers {
    const NAME: &'static str = "plant flowers";

    fn new(cell: Point, seconds: f32) -> PlantFlowers {
        PlantFlowers {
            plot: cell,
            seconds,
        }
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
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{FLOOR, Map, Point, Size, TERRAIN, TerrainId};

    const PLOT: Point = Point::new(4, 4);
    fn park() -> Map {
        Map::new(
            Size::new(8, 8),
            TerrainId(
                TERRAIN
                    .iter()
                    .position(|tile| tile.name == "grass")
                    .unwrap() as u16,
            ),
        )
    }

    #[test]
    fn planting_requests_a_flower_only_when_the_work_finishes() {
        let mut rig = Rig::new(park(), Point::new(3, 4));
        let mut task = Task::plant_flowers(PLOT, PLANT_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.run(&mut task, 1000), TaskResult::Success);
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::PLANTING, "a bed dug by hand");
        assert!(matches!(
            rig.effect,
            Effect::PlantFlowers {
                at: PLOT,
                variety: 0..=5,
                ..
            }
        ));
    }

    #[test]
    fn planting_rechecks_the_ground_and_leaves_no_flower_when_interrupted() {
        let mut rig = Rig::new(park(), Point::new(3, 4));
        let mut task = Task::plant_flowers(PLOT, PLANT_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.world.map.set_terrain(PLOT, FLOOR);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.walk.body().dirt(), crate::sim::dirt::Dirt::CLEAN, "a bed not dug soils nobody");
        let mut far = Rig::new(park(), Point::new(1, 1));
        assert_eq!(
            far.tick(&mut Task::plant_flowers(PLOT, PLANT_SECONDS)),
            TaskResult::Failed
        );
    }
}
