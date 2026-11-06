//! [`Decree`]: the king, on its throne, decides where the next outpost goes —
//! half an hour of court given to it, and then its flag is put up out in the
//! wasteland, from where the king sits (`sim::outposts`).
//!
//! The court is a [`Work`] stretch at the throne, with all its guards (on the
//! throne, nothing in the way) and its tiring; what the king decides is
//! worked out when it is over — [`choose_site`], where the biggest base of
//! scrap anybody can get to says — and asked of the world
//! ([`Effect::Decree`]), which founds the outpost, or remembers that the king
//! found nowhere today.
//!
//! The same court picks the soldiers' training ground, once and first
//! ([`ground_due`], [`choose_ground`], [`Effect::DecreeGround`]): it costs
//! nothing, and the soldiers wait on it (`sim::training`).

use crate::map::Point;
use crate::sim::biology::Biology;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;
use crate::sim::outposts::{choose_site, decree_due};
use crate::sim::training::{choose_ground, ground_due};
use crate::sim::Effect;

use super::super::action::Action;
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use super::Work;

/// Half an hour of court to decide where an outpost goes.
pub const DECREE_SECONDS: f32 = watched(30.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Decree {
    /// The throne.
    pub throne: Point,
    pub seconds: f32,
}

impl Decree {
    fn court(&self) -> Work {
        Work { desk: self.throne, seconds: self.seconds, place: FeatureKind::Throne }
    }
}

impl TaskExecutor for Decree {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let result = self.court().execute(ctx);
        if result != TaskResult::Success {
            return result;
        }
        let day = ctx.think.clock.day();
        // The soldiers' ground first: it costs nothing, and they wait on it.
        if ground_due(ctx.think) {
            let features = ctx.think.features;
            let near = features.cells_of(FeatureKind::Command).first();
            let at = near.and_then(|&near| choose_ground(ctx.think.map, ctx.think.outposts, near));
            *ctx.effect = Effect::DecreeGround { at, day };
        } else if decree_due(ctx.think) {
            *ctx.effect = Effect::Decree { at: choose_site(ctx.think.map, ctx.think.outposts), day };
        }
        result
    }

    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        self.court().interrupted(action, biology);
    }

    fn describe(&self) -> String {
        format!("decree an outpost from the throne at {}, {}", self.throne.x, self.throne.y)
    }
}

impl Task {
    /// Decide, from the throne at `throne`, where the next outpost goes.
    pub const fn decree(throne: Point, seconds: f32) -> Task {
        Task::Decree(Decree { throne, seconds })
    }
}

impl TimedTask for Decree {
    const NAME: &'static str = "decree an outpost";

    fn new(cell: Point, seconds: f32) -> Self {
        Decree { throne: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.throne
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
    use crate::sim::outposts::OUTPOST_STEEL;
    use crate::sim::testing::prop_at;

    /// A rig on its throne in a small hall with a steel rack holding
    /// `steel`, in the wasteland of seed 3.
    fn hall(steel: u32) -> Rig {
        let throne = Point::new(6, 6);
        let mut map = Map::new(Size::new(14, 12), FLOOR).with_wasteland(3);
        prop_at(&mut map, "throne", throne);
        prop_at(&mut map, "steel rack", Point::new(2, 2));
        let mut rig = Rig::new(map, throne);
        let supply = crate::map::utilities::Supply::everywhere(rig.world.map.size());
        rig.world.facilities.features = crate::sim::feature::Features::from_map(&rig.world.map, &supply);
        let _ = rig.world.facilities.bins.add(Point::new(2, 2), steel);
        rig
    }

    #[test]
    fn with_the_steel_for_an_outpost_the_king_decides_where_it_goes_once_court_is_over() {
        let mut rig = hall(OUTPOST_STEEL);
        let mut task = Task::decree(Point::new(6, 6), 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        let Effect::Decree { at: Some(center), day: 1 } = rig.effect else { panic!("{:?}", rig.effect) };
        assert!(crate::sim::outposts::is_clear(&rig.world.map, center, &rig.world.facilities.outposts));
    }

    #[test]
    fn with_soldiers_to_drill_the_king_picks_their_ground_first_and_holds_it_to_once() {
        use crate::sim::feature::FeatureKind;
        let mut rig = hall(OUTPOST_STEEL);
        rig.world.facilities.features.add(FeatureKind::Command, Point::new(4, 6));
        let mut task = Task::decree(Point::new(6, 6), 0.2);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        let Effect::DecreeGround { at: Some(center), day: 1 } = rig.effect else { panic!("{:?}", rig.effect) };
        assert!(crate::sim::training::is_clear(&rig.world.map, center, &rig.world.facilities.outposts));
        assert!(rig.world.facilities.training.found(&rig.world.map, center, &rig.world.facilities.outposts, 1));
        // The ground picked, the next half hour of court is the outpost's.
        let mut task = Task::decree(Point::new(6, 6), 0.2);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert!(matches!(rig.effect, Effect::Decree { at: Some(_), day: 1 }), "{:?}", rig.effect);
    }

    #[test]
    fn short_of_steel_the_king_only_holds_court() {
        let mut rig = hall(OUTPOST_STEEL - 1);
        let mut task = Task::decree(Point::new(6, 6), 0.2);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert_eq!(rig.effect, Effect::None);
    }
}
