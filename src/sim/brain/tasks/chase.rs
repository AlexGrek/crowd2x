//! [`Chase`]: run somebody down until they are within reach.
//!
//! A deathdog's (`HuntGoal`). Every tick it finds its quarry again where it
//! was last — the cell it was in, or one of the eight round it, since nobody
//! crosses more than a cell a tick (two round it on its first tick: what a
//! look saw may be a few ticks old). No scan of the crowd: nine reads of the
//! occupancy grid, twenty-five at the start.
//!
//! * **Lost**, or on the site, where no deathdog follows: [`TaskResult::Failed`].
//! * **In reach** (a step away, diagonals too): [`TaskResult::Success`] —
//!   once it has rested since its last bite ([`Chase::rest`]); until then it
//!   stands over its quarry and keeps track of it.
//! * **Otherwise** it runs at it, routing again only once the quarry has
//!   drifted [`DRIFT`] cells from where the route ends, so the walk is never
//!   stopped between one route and the next and the deathdog keeps up its
//!   speed — and the routes asked for are a few a second, not one a tick.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::uid::Uid;

use super::super::action::{Action, ActionState};
use super::super::task::{SavedTask, Task, TaskCtx, TaskExecutor, TaskResult};

/// How far the quarry may get from where the route ends, in a king's steps,
/// before it is routed to again.
pub const DRIFT: i32 = 2;

/// How far round where it was last seen the quarry is looked for on the
/// first tick of a chase, in a king's steps.
pub const FIRST_LOOK: i32 = 2;

/// Who is being chased, told apart by the low 32 bits of its uid's random
/// half: a whole uid is eight bytes aligned to eight, and a [`Task`] holding
/// one is four bytes more — times the ten tasks a brain holds, in every
/// person. It is
/// only ever compared with whoever stands within two cells of where the
/// quarry was, so two that share it there is a chance in hundreds of
/// millions, and would only set a deathdog on the wrong person.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Quarry(u32);

impl Quarry {
    fn of(uid: Uid) -> Quarry {
        Quarry(uid.body() as u32)
    }

    fn is(self, uid: Uid) -> bool {
        self.0 == uid.body() as u32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chase {
    quarry: Quarry,
    /// Where the quarry was last found.
    pub at: Point,
    /// Ticks before it may end in a bite: a deathdog rests between bites
    /// (`deathdogs::BITE_INTERVAL`). Ticks, not seconds, for the size of the
    /// task — the goal works them out from the tick's length.
    pub rest: u16,
}

impl Chase {
    /// Whether `uid` is the one being chased.
    pub fn is_after(&self, uid: Uid) -> bool {
        self.quarry.is(uid)
    }

    /// Where the quarry is now, looking `radius` round where it was.
    fn find(&self, ctx: &TaskCtx<'_>, radius: i32) -> Option<Point> {
        let occupancy = ctx.think.occupancy;
        let quarry = |cell: Point| occupancy.occupant(cell).is_some_and(|uid| self.quarry.is(uid));
        if quarry(self.at) {
            return Some(self.at);
        }
        (-radius..=radius)
            .flat_map(|dy| (-radius..=radius).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| self.at.offset(dx, dy))
            .find(|&cell| quarry(cell))
    }
}

impl TaskExecutor for Chase {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let first = ctx.action.is_none();
        let Some(found) = self.find(ctx, if first { FIRST_LOOK } else { 1 }) else {
            return TaskResult::Failed;
        };
        self.at = found;
        if ctx.think.map.site().contains(found) {
            return TaskResult::Failed;
        }
        self.rest = self.rest.saturating_sub(1);
        if ctx.here().chebyshev_distance(found) <= 1 {
            if self.rest == 0 {
                return TaskResult::Success;
            }
            if !matches!(ctx.action, Action::Wait { .. }) {
                ctx.walk.halt();
                *ctx.action = Action::wait(self.rest as f32 * ctx.think.dt);
            }
            return TaskResult::Executing;
        }
        let reroute = match ctx.action {
            Action::Move { .. } => ctx.walk.destination().is_none_or(|end| end.chebyshev_distance(found) >= DRIFT),
            _ => true,
        };
        if reroute {
            return match Action::walk_to(ctx.walk, ctx.think, found) {
                Some(walk) => {
                    *ctx.action = walk;
                    TaskResult::Executing
                }
                None => TaskResult::Failed,
            };
        }
        match ctx.advance_action() {
            ActionState::Failed => TaskResult::Failed,
            ActionState::Running | ActionState::Finished => TaskResult::Executing,
        }
    }

    fn describe(&self) -> String {
        format!("chase {:08x} at {}, {}", self.quarry.0, self.at.x, self.at.y)
    }
}

impl Task {
    /// Run `quarry` down from where it was last seen, `at`, and end within
    /// reach of it once `rest` ticks have gone by.
    pub fn chase(quarry: Uid, at: Point, rest: u16) -> Task {
        Task::Chase(Chase { quarry: Quarry::of(quarry), at, rest })
    }
}

/// Saved as a chase of its own (`pb::Chase`): who, where, and the rest left.
impl SavedTask for Chase {
    fn save(&self) -> pb::TaskKind {
        let Chase { quarry: Quarry(quarry), at, rest } = *self;
        pb::TaskKind::Chase(pb::Chase { quarry, at: Some(at.into()), rest: rest as u32 })
    }

    fn restore(saved: &pb::TaskKind) -> Option<Result<Chase, Invalid>> {
        let pb::TaskKind::Chase(chase) = saved else {
            return None;
        };
        Some((|| {
            let rest = u16::try_from(chase.rest).map_err(|_| Invalid::new(format!("a chase resting {} ticks", chase.rest)))?;
            Ok(Chase { quarry: Quarry(chase.quarry), at: pb::cell(chase.at, "chase")?, rest })
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::uid::EntityType;

    fn quarry() -> Uid {
        Uid::new(EntityType::Human, 0x00ab_cdef_0123_4567)
    }

    /// The rig's own world is all site, and a chase gives up on anybody on
    /// the site; so this one is a wasteland round a site of one cell.
    fn rig(from: Point) -> Rig {
        let map = Map::new(Size::new(1, 1), FLOOR).with_wasteland(5);
        let mut rig = Rig::new(map, from);
        rig.world.map.load_chunks(rig.world.map.navigation_area());
        rig
    }

    #[test]
    fn a_chase_knows_its_quarry_and_comes_back_from_a_save_as_itself() {
        let task = Task::chase(quarry(), Point::new(-3, 4), 7);
        let Task::Chase(chase) = task else { unreachable!() };
        assert!(chase.is_after(quarry()));
        assert!(!chase.is_after(Uid::new(EntityType::Human, 0x00ab_cdef_0123_4568)));
        assert_eq!(Task::restore(&task.save()).unwrap(), task);
    }

    /// The point of keeping the quarry in four bytes: a chase is no bigger
    /// than the biggest task before it, so a task is no bigger in anybody.
    #[test]
    fn a_chase_is_no_bigger_than_a_task_was() {
        assert!(std::mem::size_of::<Chase>() <= 16, "{}", std::mem::size_of::<Chase>());
        assert!(std::mem::size_of::<Task>() <= 20, "{}", std::mem::size_of::<Task>());
    }

    #[test]
    fn it_runs_a_quarry_down_and_ends_in_reach_once_rested() {
        let at = Point::new(-12, -10);
        let mut rig = rig(Point::new(-20, -10));
        let _ = rig.world.occupancy.claim(at, quarry());
        let mut task = Task::chase(quarry(), at, 0);
        assert_eq!(rig.run(&mut task, 2000), TaskResult::Success);
        assert!(rig.walk.body().center_position().chebyshev_distance(at) <= 1);

        let mut resting = Task::chase(quarry(), at, 30);
        for tick in 0..29 {
            assert_eq!(rig.tick(&mut resting), TaskResult::Executing, "tick {tick}: still resting");
        }
        assert_eq!(rig.tick(&mut resting), TaskResult::Success);
    }

    #[test]
    fn it_follows_a_quarry_that_moves_a_cell_and_fails_once_it_is_gone() {
        let mut rig = rig(Point::new(-20, -10));
        let _ = rig.world.occupancy.claim(Point::new(-10, -10), quarry());
        let mut task = Task::chase(quarry(), Point::new(-10, -10), 0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.world.occupancy.release(Point::new(-10, -10), quarry());
        let _ = rig.world.occupancy.claim(Point::new(-10, -9), quarry());
        assert_eq!(rig.tick(&mut task), TaskResult::Executing, "a cell over is still in sight");
        let Task::Chase(chase) = task else { unreachable!() };
        assert_eq!(chase.at, Point::new(-10, -9));
        rig.world.occupancy.release(Point::new(-10, -9), quarry());
        assert_eq!(rig.tick(&mut task), TaskResult::Failed, "gone");
    }

    #[test]
    fn a_quarry_on_the_site_is_given_up_on() {
        let mut rig = rig(Point::new(-3, 0));
        let _ = rig.world.occupancy.claim(Point::new(0, 0), quarry());
        assert_eq!(rig.tick(&mut Task::chase(quarry(), Point::new(0, 0), 0)), TaskResult::Failed);
    }
}

