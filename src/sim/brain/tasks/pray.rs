//! [`Pray`]: pray beside a pew in the church.
//!
//! A place and a time, like any timed task: stand beside the pew for
//! [`PRAYER_SECONDS`](crate::sim::religion::PRAYER_SECONDS), and on finishing
//! tell the body it prayed ([`Event::Prayed`]) — how devout the one praying
//! is, its religiosity, and whether a cleric was leading prayers at the
//! church's altar on the tick it ended, a service
//! ([`Services::is_on`](crate::sim::religion::Services::is_on)). What that
//! does to the mind is the `Mind` process's. A prayer cut short is nothing.

use crate::map::Point;
use crate::sim::biology::Event;
use crate::sim::feature::FeatureKind;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pray {
    /// The pew prayed at.
    pub pew: Point,
    /// How long the prayer takes.
    pub seconds: f32,
}

impl TaskExecutor for Pray {
    /// **From beside the pew, every tick**, a pew still there, and a body to
    /// pray with.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.pew) > 1
            || ctx.biology.is_none()
            || !ctx.think.features.has(FeatureKind::Pew, self.pew)
        {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.pew, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let devotion = ctx.walk.body().religiosity() as f32 / 100.0;
                let service = ctx.think.services.is_on(self.pew);
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Prayed { devotion, service });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("pray at the pew at {}, {}", self.pew.x, self.pew.y)
    }
}

impl Task {
    /// Pray beside the pew in `pew` for `seconds`.
    pub const fn pray(pew: Point, seconds: f32) -> Task {
        Task::Pray(Pray { pew, seconds })
    }
}

/// Saved as `pray`: the pew and the seconds.
impl TimedTask for Pray {
    const NAME: &'static str = "pray";

    fn new(cell: Point, seconds: f32) -> Pray {
        Pray { pew: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.pew
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
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::biology::mind::{PRAYER_PEACE, SERVICE_BLESSING};
    use crate::sim::testing::prop_at;

    const PEW: Point = Point::new(4, 4);
    const ALTAR: Point = Point::new(4, 7);

    fn church() -> Map {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        prop_at(&mut map, "pew", PEW);
        prop_at(&mut map, "altar", ALTAR);
        map
    }

    /// A rig beside the pew, of `religiosity`, its mind at 50.
    fn praying(religiosity: i32) -> Rig {
        let mut rig = Rig::new(church(), PEW.offset(0, -1));
        rig.walk.body_mut().set_religiosity(religiosity);
        rig.biology.edit(|stats| stats.with_mental_health(50.0));
        rig
    }

    #[test]
    fn praying_from_across_the_church_fails_without_starting() {
        let mut rig = Rig::new(church(), Point::new(1, 1));
        assert_eq!(rig.tick(&mut Task::pray(PEW, 1.0)), TaskResult::Failed);
    }

    /// Nothing until it is over, then peace by how devout: all of it for the
    /// most religious, half for somebody half as religious.
    #[test]
    fn a_prayer_mends_the_mind_only_once_it_is_said_and_by_how_devout_the_one_praying_is() {
        for (religiosity, peace) in [(100, PRAYER_PEACE), (50, PRAYER_PEACE / 2.0)] {
            let mut rig = praying(religiosity);
            let mut task = Task::pray(PEW, 1.0);
            assert_eq!(rig.tick(&mut task), TaskResult::Executing);
            assert_eq!(rig.biology.stats().mental_health(), 50.0, "not before it is said");
            assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
            assert!((rig.biology.stats().mental_health() - (50.0 + peace)).abs() < 1e-4, "{religiosity}: {}", rig.biology.stats().mental_health());
        }
    }

    /// With a cleric leading prayers at the altar as it ends, a prayer is a
    /// service, and worth twice as much.
    #[test]
    fn a_prayer_at_a_service_is_worth_twice_a_plain_one() {
        let mut rig = praying(100);
        let mut task = Task::pray(PEW, 1.0);
        assert!(rig.world.services.lead(ALTAR));
        assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
        let mended = rig.biology.stats().mental_health() - 50.0;
        assert!((mended - PRAYER_PEACE * SERVICE_BLESSING).abs() < 1e-4, "{mended}");
    }

    #[test]
    fn walking_away_mid_prayer_fails_it_and_mends_nothing() {
        let mut rig = praying(100);
        let mut task = Task::pray(PEW, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().mental_health(), 50.0);
    }
}
