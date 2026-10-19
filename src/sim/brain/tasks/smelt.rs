//! [`Smelt`]: make an ingot of steel at a furnace from scrap metal in the
//! pack.
//!
//! A steelworker's piece of work (`SmeltGoal`): from beside its furnace, half
//! an hour of world, and [`SCRAP_PER_STEEL`] pieces of scrap metal go in and
//! an ingot of steel comes out. All of it the unit's own pack, so nothing is
//! asked of the world. The furnace burns while it is worked: the task's
//! action names it ([`Action::interact`]), which is what lights it on screen.
//!
//! It is work, and tires like it ([`Event::Worked`]): for the time it took,
//! or for as much as was done of it if it is cut short — when nothing comes
//! of it, the scrap staying in the pack.

use crate::map::Point;
use crate::save::pb;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::feature::FeatureKind;
use crate::sim::item::{ItemKind, SCRAP_PER_STEEL};

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Half an hour of world at the furnace for an ingot.
pub const SMELT_SECONDS: f32 = watched(30.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Smelt {
    /// The furnace's cell: what the unit is turned towards, never where it
    /// stands.
    pub furnace: Point,
    /// How long an ingot takes, in **watched** seconds.
    pub seconds: f32,
}

impl Smelt {
    /// Whether it can go on: one step from the furnace, the furnace working
    /// (it needs power), and enough scrap in the pack for an ingot.
    fn can_smelt(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.here().manhattan_distance(self.furnace) <= 1
            && ctx.think.features.has(FeatureKind::Furnace, self.furnace)
            && ctx.inventory.as_deref().is_some_and(|inventory| inventory.count(ItemKind::Scrap) >= SCRAP_PER_STEEL)
    }
}

impl TaskExecutor for Smelt {
    /// **Checked every tick**: a unit pulled away, a furnace switched off or
    /// a pack without the scrap ends the work there, and nothing is made.
    /// Finishing takes the scrap out of the pack and puts the ingot in.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_smelt(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.furnace, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let Some(inventory) = ctx.inventory.as_deref_mut() else {
                    return TaskResult::Failed;
                };
                for _ in 0..SCRAP_PER_STEEL {
                    let _ = inventory.take_out(ItemKind::Scrap);
                }
                // The scrap out weighs more than the ingot in, so there is
                // always room for it.
                let _ = inventory.stow(ItemKind::Steel);
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

    /// Pulled away before the ingot was made: nothing came of it, but the
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
        format!("smelt steel at the furnace at {}, {}", self.furnace.x, self.furnace.y)
    }
}

impl Task {
    /// Smelt scrap in the pack into an ingot of steel at the furnace in
    /// `furnace`, from a cell beside it.
    pub const fn smelt(furnace: Point, seconds: f32) -> Task {
        Task::Smelt(Smelt { furnace, seconds })
    }
}

/// Saved as `smelt`: the furnace's cell and the seconds.
impl TimedTask for Smelt {
    const NAME: &'static str = "smelt";

    fn new(cell: Point, seconds: f32) -> Smelt {
        Smelt { furnace: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.furnace
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }

    fn legacy(saved: &pb::TaskKind) -> Option<&pb::Timed> {
        match saved {
            pb::TaskKind::Smelt(task) => Some(task),
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

    const FURNACE: Point = Point::new(6, 6);

    /// A unit in `cell` of a room with a working furnace at [`FURNACE`],
    /// carrying `pieces` of scrap metal.
    fn rig_at(cell: Point, pieces: u8) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "furnace", FURNACE);
        let mut rig = Rig::new(map, cell);
        rig.biology.edit(|stats| stats.with_stamina(80.0).with_fun(40.0));
        for _ in 0..pieces {
            assert!(rig.inventory.stow(ItemKind::Scrap));
        }
        rig
    }

    /// Two pieces of scrap go in and an ingot comes out, only once the work
    /// is over; it tires, and it is no fun; the furnace is what is worked.
    #[test]
    fn two_pieces_of_scrap_become_an_ingot_of_steel_once_the_work_is_over() {
        let mut rig = rig_at(Point::new(5, 6), 3);
        let mut task = Task::smelt(FURNACE, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.action.interacting_with(), Some(FURNACE), "the furnace burns while it is worked");
        assert_eq!(rig.inventory.count(ItemKind::Steel), 0, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!((rig.inventory.count(ItemKind::Scrap), rig.inventory.count(ItemKind::Steel)), (1, 1));
        assert!(rig.biology.stats().stamina() < 80.0, "smelting is work");
        assert_eq!(rig.biology.stats().fun(), 40.0, "and no fun");
    }

    #[test]
    fn one_piece_of_scrap_is_not_enough_for_an_ingot() {
        let mut rig = rig_at(Point::new(5, 6), 1);
        assert_eq!(rig.tick(&mut Task::smelt(FURNACE, 0.5)), TaskResult::Failed);
        assert!(rig.action.is_none());
    }

    #[test]
    fn a_furnace_out_of_reach_or_without_power_smelts_nothing() {
        let mut rig = rig_at(Point::new(1, 1), 2);
        assert_eq!(rig.tick(&mut Task::smelt(FURNACE, 0.5)), TaskResult::Failed);

        let mut rig = rig_at(Point::new(5, 6), 2);
        let mut task = Task::smelt(FURNACE, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let dead = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.features = crate::sim::feature::Features::from_map(&rig.world.map, &dead);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.inventory.count(ItemKind::Scrap), 2, "nothing went in");
    }
}
