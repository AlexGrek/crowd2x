//! [`Lay`]: a builder lays the building tile of the tile it has taken on, from
//! beside it — all the steel it takes at once (`sim::outposts`).
//!
//! A few minutes' work; the plan and the ground are the world's, so the task
//! *asks* ([`Effect::Lay`]) when it is done, and the world step lays it —
//! unless somebody has stepped into the cell meanwhile, or a body lies there.
//! The builder's goal sees next tick whether it was laid.
//!
//! It is work, and tires like it ([`Event::Worked`]): for the time it took,
//! or for as much as was done of it if it is cut short.

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::item::ItemKind;
use crate::sim::outposts::Stage;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Five minutes of world to lay a building tile.
pub const LAY_SECONDS: f32 = watched(5.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lay {
    /// The tile's cell: what the builder is turned towards, never where it
    /// stands.
    pub tile: Point,
    /// How long laying it takes, in **watched** seconds.
    pub seconds: f32,
}

impl Lay {
    /// Whether it can go on: one step from the tile, not in it, the tile the
    /// builder's own and only on the plan still, and the steel it takes in
    /// the pack.
    fn can_lay(&self, ctx: &TaskCtx<'_>) -> bool {
        let here = ctx.here();
        let uid = ctx.walk.body().uid();
        let Some(tile) = ctx.think.outposts.claim_of(uid).filter(|tile| tile.cell == self.tile) else {
            return false;
        };
        let steel = ctx.inventory.as_deref().map_or(0, |pack| pack.count(ItemKind::Steel) as u32);
        here.manhattan_distance(self.tile) == 1 && tile.stage == Stage::Planned && steel >= tile.steel_wanted()
    }
}

impl TaskExecutor for Lay {
    /// **Checked every tick**: a builder pulled away, a tile no longer its
    /// own or the steel gone ends it there, and nothing is asked for.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_lay(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.tile, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::Lay { at: self.tile };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before it was down: nothing laid, but the time spent at it
    /// was still work.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("lay a building tile at {}, {}", self.tile.x, self.tile.y)
    }
}

impl Task {
    /// Lay the building tile of one's own tile at `tile`, from beside it.
    pub const fn lay(tile: Point, seconds: f32) -> Task {
        Task::Lay(Lay { tile, seconds })
    }
}

impl TimedTask for Lay {
    const NAME: &'static str = "lay a building tile";

    fn new(cell: Point, seconds: f32) -> Self {
        Lay { tile: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.tile
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::outposts::{first_clear_site, Part};

    /// A rig beside the first wall of an outpost founded off a small site,
    /// the wall its own.
    fn beside_a_wall() -> (Rig, Point) {
        let map = Map::new(Size::new(12, 10), FLOOR).with_wasteland(3);
        let center = first_clear_site(&map);
        let mut outposts = crate::sim::outposts::Outposts::new();
        assert!(outposts.found(&map, center, 1));
        let tile = *outposts.under_way().unwrap().tiles().iter().find(|tile| tile.part == Part::Wall).unwrap();
        let stand = outposts.stand_cells(tile.cell).next().unwrap();
        let mut rig = Rig::new(map, stand);
        rig.world.facilities.outposts = outposts;
        let uid = rig.walk.body().uid();
        assert!(rig.world.facilities.outposts.claim(tile.cell, uid));
        (rig, tile.cell)
    }

    #[test]
    fn a_building_tile_is_asked_for_once_it_is_laid_and_only_with_the_steel() {
        let (mut rig, wall) = beside_a_wall();
        let mut task = Task::lay(wall, 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed, "no steel");
        assert!(rig.inventory.stow(ItemKind::Steel));
        let stamina = rig.biology.stats().stamina();
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None, "not before it is done");
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Lay { at: wall });
        assert!(rig.biology.stats().stamina() < stamina, "it is work");
    }

    #[test]
    fn a_tile_not_ones_own_or_out_of_reach_is_not_laid() {
        let (mut rig, wall) = beside_a_wall();
        assert!(rig.inventory.stow(ItemKind::Steel));
        let uid = rig.walk.body().uid();
        rig.world.facilities.outposts.release(uid);
        assert_eq!(rig.tick(&mut Task::lay(wall, 0.2)), TaskResult::Failed, "not its own");
        assert!(rig.world.facilities.outposts.claim(wall, uid));
        let far = wall.offset(3, 3);
        assert_eq!(rig.tick(&mut Task::lay(far, 0.2)), TaskResult::Failed, "not its tile");
    }
}
