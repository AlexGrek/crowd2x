//! [`Claim`]: a builder takes on a tile of the outpost going up — the
//! building plan's (`sim::outposts`), which every builder reads, and in which
//! no two builders ever have one tile.
//!
//! No action, nothing walked to: the plan is the world's, so the task *asks*
//! ([`Effect::Claim`]) the tick it starts, and the world step hands the tile
//! out in slot order. The builder's goal reads next tick whether it got it.

use crate::map::Point;
use crate::sim::outposts::Stage;
use crate::sim::Effect;

use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Claim {
    /// The tile's cell.
    pub tile: Point,
}

impl TaskExecutor for Claim {
    /// Asked once, at once: fails — and asks nothing — where the tile is gone,
    /// built, somebody else's, or the builder has one already.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let uid = ctx.walk.body().uid();
        let outposts = ctx.think.outposts;
        let free = outposts.tile(self.tile).is_some_and(|tile| tile.claim.is_none() && tile.stage != Stage::Built);
        if !free || outposts.claim_of(uid).is_some() {
            return TaskResult::Failed;
        }
        *ctx.effect = Effect::Claim { at: self.tile };
        TaskResult::Success
    }

    fn describe(&self) -> String {
        format!("take on the outpost's tile at {}, {}", self.tile.x, self.tile.y)
    }
}

impl Task {
    /// Take on the tile of the outpost going up at `tile`.
    pub const fn claim(tile: Point) -> Task {
        Task::Claim(Claim { tile })
    }
}

impl TimedTask for Claim {
    const NAME: &'static str = "take on a tile";

    fn new(cell: Point, _seconds: f32) -> Self {
        Claim { tile: cell }
    }

    fn cell(&self) -> Point {
        self.tile
    }

    fn seconds(&self) -> f32 {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::outposts::first_clear_site;

    #[test]
    fn a_builder_asks_for_a_free_tile_and_nothing_else() {
        let map = Map::new(Size::new(12, 10), FLOOR).with_wasteland(3);
        let center = first_clear_site(&map);
        let mut rig = Rig::new(map, Point::new(2, 2));
        assert!(rig.world.facilities.outposts.found(&rig.world.map, center, 1));
        let tile = rig.world.facilities.outposts.pick(Point::new(2, 2), 0, false).unwrap();
        let mut task = Task::claim(tile);
        assert_eq!(rig.tick(&mut task), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Claim { at: tile });

        let uid = rig.walk.body().uid();
        assert!(rig.world.facilities.outposts.claim(tile, uid));
        let mut again = Task::claim(rig.world.facilities.outposts.pick(Point::new(2, 2), 0, false).unwrap());
        assert_eq!(rig.tick(&mut again), TaskResult::Failed, "one tile to a builder");
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.tick(&mut Task::claim(center)), TaskResult::Failed, "nothing to build under the flag");
    }
}
