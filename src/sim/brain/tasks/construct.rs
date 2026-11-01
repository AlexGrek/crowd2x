//! [`Construct`]: a builder works on the building tile of its own tile, from
//! beside it, for a stretch — an hour at the most (`sim::outposts::STRETCH`).
//!
//! The tile's progress is the world's, so **every tick it works, the task
//! asks** ([`Effect::Construct`]) for that tick's worth of world time to be
//! counted: a stretch cut short — called away, a fright, the end of the day —
//! has counted all it did, and nothing is lost but the tick. Once the tile has
//! had all it takes (`TILE_WORK`), the world builds it and gives it up, and
//! the task ends there, done.
//!
//! It is work, and tires like it ([`Event::Worked`]): for the stretch, or for
//! as much of it as was worked.

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::TIME_SCALE;
use crate::sim::outposts::Stage;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Construct {
    /// The tile's cell: what the builder is turned towards, never where it
    /// stands.
    pub tile: Point,
    /// How long the stretch is, in **watched** seconds.
    pub seconds: f32,
}

impl Construct {
    /// Whether it can go on: one step from the tile, not in it, and the tile
    /// the builder's own with its building tile down.
    fn can_work(&self, ctx: &TaskCtx<'_>) -> bool {
        let uid = ctx.walk.body().uid();
        ctx.here().manhattan_distance(self.tile) == 1
            && ctx
                .think
                .outposts
                .claim_of(uid)
                .is_some_and(|tile| tile.cell == self.tile && matches!(tile.stage, Stage::Laid { .. }))
    }

    /// Tell the body about the work done so far in this stretch.
    fn worked(action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }
}

impl TaskExecutor for Construct {
    /// **Checked every tick.** The tile built since the last — the work it
    /// took all done — is the stretch done, and a success; pulled away, or
    /// the tile no longer its own, a failure.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_work(ctx) {
            let built = ctx.think.outposts.iter().last().and_then(|outpost| outpost.tile(self.tile)).is_some_and(|tile| tile.stage == Stage::Built);
            if built && matches!(ctx.action, Action::Interact { .. }) {
                Self::worked(ctx.action, ctx.biology.as_deref_mut());
                *ctx.action = Action::None;
                return TaskResult::Success;
            }
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.tile, self.seconds);
            return TaskResult::Executing;
        }
        let state = ctx.advance_action();
        if state != ActionState::Failed {
            *ctx.effect = Effect::Construct { at: self.tile, seconds: ctx.think.dt * TIME_SCALE };
        }
        match state {
            ActionState::Finished => {
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away mid-stretch: the world has counted the work already; the
    /// body is told of it here.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        Self::worked(action, biology);
    }

    fn describe(&self) -> String {
        format!("build at {}, {}", self.tile.x, self.tile.y)
    }
}

impl Task {
    /// Work on the building tile of one's own tile at `tile`, from beside it,
    /// for a stretch of `seconds`.
    pub const fn construct(tile: Point, seconds: f32) -> Task {
        Task::Construct(Construct { tile, seconds })
    }
}

impl TimedTask for Construct {
    const NAME: &'static str = "build";

    fn new(cell: Point, seconds: f32) -> Self {
        Construct { tile: cell, seconds }
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
    use crate::sim::outposts::{first_clear_site, Outposts, Part, TILE_WORK};

    /// A rig beside the first wall of an outpost founded off a small site,
    /// the wall its own and its building tile down.
    fn beside_a_laid_wall() -> (Rig, Point) {
        let map = Map::new(Size::new(12, 10), FLOOR).with_wasteland(3);
        let center = first_clear_site(&map);
        let mut outposts = Outposts::new();
        assert!(outposts.found(&map, center, 1));
        let tile = *outposts.under_way().unwrap().tiles().iter().find(|tile| tile.part == Part::Wall).unwrap();
        let stand = outposts.stand_cells(tile.cell).next().unwrap();
        let mut rig = Rig::new(map, stand);
        rig.world.map.load_chunks(crate::map::Bounds::new(tile.cell, Size::new(1, 1)).grown(1));
        rig.world.facilities.outposts = outposts;
        let uid = rig.walk.body().uid();
        assert!(rig.world.facilities.outposts.claim(tile.cell, uid));
        rig.world.facilities.outposts.lay(&mut rig.world.map, tile.cell, uid).unwrap();
        (rig, tile.cell)
    }

    #[test]
    fn every_tick_of_a_stretch_asks_for_its_work_to_be_counted() {
        let (mut rig, wall) = beside_a_laid_wall();
        let mut task = Task::construct(wall, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None, "starting is not working");
        let stamina = rig.biology.stats().stamina();
        let mut counted = 0.0;
        let mut result = TaskResult::Executing;
        for _ in 0..1000 {
            result = rig.tick(&mut task);
            let Effect::Construct { at, seconds } = rig.effect else { panic!("{:?}", rig.effect) };
            assert_eq!(at, wall);
            counted += seconds;
            if result.is_finished() {
                break;
            }
        }
        assert_eq!(result, TaskResult::Success);
        assert!((counted - TIME_SCALE).abs() <= rig.world.dt * TIME_SCALE + 0.01, "a stretch counts what it worked: {counted}");
        assert!(rig.biology.stats().stamina() < stamina, "it is work");
    }

    #[test]
    fn the_tile_built_mid_stretch_ends_it_done_and_walking_away_fails_it() {
        let (mut rig, wall) = beside_a_laid_wall();
        let uid = rig.walk.body().uid();
        let mut task = Task::construct(wall, 100.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let done = rig.world.facilities.outposts.construct(&mut rig.world.map, wall, uid, TILE_WORK);
        assert!(done.is_some());
        assert_eq!(rig.tick(&mut task), TaskResult::Success, "built: the stretch is over");
        assert_eq!(rig.effect, Effect::None);

        let (mut rig, wall) = beside_a_laid_wall();
        let mut task = Task::construct(wall.offset(5, 5), 100.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed, "not beside its own tile");
    }
}
