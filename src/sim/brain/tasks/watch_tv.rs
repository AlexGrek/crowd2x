//! [`WatchTv`]: stand within sight of a TV and watch it.
//!
//! [`UseComputer`](super::UseComputer)'s twin, but from across the room: a
//! TV is [`Access::Watched`](crate::sim::feature::Access::Watched), so the
//! unit may stand anywhere up to
//! [`VIEWING_DISTANCE`](crate::sim::feature::VIEWING_DISTANCE) from it with
//! nothing but see-through terrain between them
//! ([`can_watch`](crate::sim::feature::can_watch)). What the body gets out of
//! it is the same [`Event::Entertained`], once the programme is over.

use crate::map::Point;
use crate::sim::biology::Event;
use crate::sim::feature::{can_watch, FeatureKind};

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WatchTv {
    /// The TV's cell: what the unit is turned towards, and — through
    /// [`Action::interact`] — what shows its picture while somebody watches.
    pub tv: Point,
    /// How long a programme takes.
    pub seconds: f32,
}

impl TaskExecutor for WatchTv {
    /// **Only from where the picture can be seen, checked every tick**: near
    /// enough, in sight, and the set working. This task does not walk; a unit
    /// pushed out of sight, or a box switched off, ends the programme
    /// unfinished, and half a programme is no fun at all.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.biology.is_none()
            || !can_watch(ctx.think.map, ctx.here(), self.tv)
            || !ctx.think.features.has(FeatureKind::Television, self.tv)
        {
            return TaskResult::Failed;
        }

        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.tv, self.seconds);
            return TaskResult::Executing;
        }

        match ctx.advance_action() {
            ActionState::Finished => {
                let recalled = ctx.biology.as_deref_mut().and_then(|b| b.handle(Event::Entertained));
                super::report_recalled(ctx, recalled);
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("watch the TV at {}, {}", self.tv.x, self.tv.y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR, WALL};

    const TV: Point = Point::new(6, 6);

    /// A unit in `cell` of a room with a working TV at [`TV`].
    fn rig_at(cell: Point, map: Option<Map>) -> Rig {
        let mut map = map.unwrap_or_else(|| Map::new(Size::new(12, 12), FLOOR));
        crate::sim::testing::prop_at(&mut map, "tv", TV);
        let mut rig = Rig::new(map, cell);
        rig.biology.edit(|stats| stats.with_fun(20.0));
        rig
    }

    #[test]
    fn a_programme_watched_from_three_cells_away_is_worth_something_once_it_is_over() {
        let mut rig = rig_at(Point::new(6, 3), None);
        let mut task = Task::watch_tv(TV, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.biology.stats().fun(), 20.0, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert!(rig.biology.stats().fun() > 20.0, "{}", rig.biology.stats().fun());
        assert_eq!(rig.walk.body().center_position(), Point::new(6, 3), "watched from where it stood");
    }

    #[test]
    fn a_tv_four_cells_away_is_too_far_to_watch() {
        let mut rig = rig_at(Point::new(6, 2), None);
        let mut task = Task::watch_tv(TV, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }

    #[test]
    fn a_tv_behind_a_wall_cannot_be_watched_however_near() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        map.set_terrain(Point::new(6, 5), WALL);
        let mut rig = rig_at(Point::new(6, 4), Some(map));
        let mut task = Task::watch_tv(TV, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }

    #[test]
    fn walking_out_of_sight_mid_programme_fails_it_and_is_no_fun_at_all() {
        let mut rig = rig_at(Point::new(4, 6), None);
        let mut task = Task::watch_tv(TV, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }

    #[test]
    fn losing_power_mid_programme_ends_it() {
        let mut rig = rig_at(Point::new(5, 6), None);
        let mut task = Task::watch_tv(TV, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let dead = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.features = crate::sim::feature::Features::from_map(&rig.world.map, &dead);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }
}
