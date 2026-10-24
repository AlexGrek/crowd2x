//! One account's payroll, done beside a powered office desk.

use crate::map::Point;
use crate::sim::biology::Biology;
use crate::sim::feature::FeatureKind;
use crate::sim::Effect;
use super::super::action::Action;
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use super::Work;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Payroll {
    pub desk: Point,
    pub seconds: f32,
}

impl TaskExecutor for Payroll {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let result = Work { desk: self.desk, seconds: self.seconds, place: FeatureKind::Workstation }.execute(ctx);
        if result == TaskResult::Success {
            *ctx.effect = Effect::Payroll { at: self.desk };
        }
        result
    }

    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        Work { desk: self.desk, seconds: self.seconds, place: FeatureKind::Workstation }.interrupted(action, biology);
    }

    fn describe(&self) -> String {
        format!("do the payroll at {}, {}", self.desk.x, self.desk.y)
    }
}

impl Task {
    pub const fn payroll(desk: Point, seconds: f32) -> Task {
        Task::Payroll(Payroll { desk, seconds })
    }
}

impl TimedTask for Payroll {
    const NAME: &'static str = "do the payroll";
    fn new(desk: Point, seconds: f32) -> Self { Self { desk, seconds } }
    fn cell(&self) -> Point { self.desk }
    fn seconds(&self) -> f32 { self.seconds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::rig::Rig;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::testing::prop_at;

    #[test]
    fn payroll_requires_a_powered_desk_and_only_pays_at_completion() {
        let desk = Point::new(4, 4);
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        prop_at(&mut map, "computer", desk);
        let mut rig = Rig::new(map, Point::new(3, 4));
        let mut task = Task::payroll(desk, 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        let stamina = rig.biology.stats().stamina();
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Payroll { at: desk });
        assert!(rig.biology.stats().stamina() < stamina);

        rig.action = Action::None;
        let mut task = Task::payroll(desk, 2.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        for _ in 0..10 { rig.tick(&mut task); }
        let stamina = rig.biology.stats().stamina();
        task.interrupted(&rig.action, Some(&mut rig.biology));
        assert!(rig.biology.stats().stamina() < stamina);
        assert_eq!(rig.effect, Effect::None, "interrupted work pays nobody");
        let supply = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.features = crate::sim::feature::Features::from_map(&rig.world.map, &supply);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed, "power lost mid-account");
        assert_eq!(rig.effect, Effect::None);
    }
}
