//! [`WriteNews`]: the propagandist, at its computer in the palace, makes the
//! oldest story waiting into an article on everybody's phone — half an hour
//! of copy written, and then it goes up (`sim::news`).
//!
//! The writing is a [`Work`] stretch at the propaganda computer, with all its
//! guards (beside it, the power on) and its tiring; the article is asked of
//! the world once it is over ([`Effect::Publish`]), which takes whatever
//! story is oldest then.

use crate::map::Point;
use crate::sim::biology::Biology;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;
use crate::sim::Effect;

use super::super::action::Action;
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use super::Work;

/// Half an hour of copy for one article.
pub const NEWS_SECONDS: f32 = watched(30.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WriteNews {
    /// The propaganda computer.
    pub desk: Point,
    pub seconds: f32,
}

impl WriteNews {
    fn copy(&self) -> Work {
        Work { desk: self.desk, seconds: self.seconds, place: FeatureKind::PropagandaComputer }
    }
}

impl TaskExecutor for WriteNews {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let result = self.copy().execute(ctx);
        if result == TaskResult::Success && ctx.think.news.waiting() > 0 {
            *ctx.effect = Effect::Publish { when: ctx.think.clock };
        }
        result
    }

    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        self.copy().interrupted(action, biology);
    }

    fn describe(&self) -> String {
        format!("write the news at the propaganda computer at {}, {}", self.desk.x, self.desk.y)
    }
}

impl Task {
    /// Write the oldest story waiting up as the news, at the propaganda
    /// computer at `desk`.
    pub const fn write_news(desk: Point, seconds: f32) -> Task {
        Task::WriteNews(WriteNews { desk, seconds })
    }
}

impl TimedTask for WriteNews {
    const NAME: &'static str = "write the news";

    fn new(cell: Point, seconds: f32) -> Self {
        WriteNews { desk: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.desk
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
    use crate::sim::news::Story;
    use crate::sim::testing::prop_at;

    /// A rig beside a powered propaganda computer.
    fn office() -> Rig {
        let desk = Point::new(4, 4);
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        prop_at(&mut map, "propaganda computer", desk);
        let mut rig = Rig::new(map, Point::new(4, 5));
        let supply = crate::map::utilities::Supply::everywhere(rig.world.map.size());
        rig.world.facilities.features = crate::sim::feature::Features::from_map(&rig.world.map, &supply);
        rig
    }

    #[test]
    fn with_a_story_waiting_the_article_goes_up_once_the_copy_is_written() {
        let mut rig = office();
        rig.world.facilities.news.report(Story::Record { item: crate::sim::item::ItemKind::Steel, total: 100 });
        let mut task = Task::write_news(Point::new(4, 4), 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None, "not before it is written");
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert!(matches!(rig.effect, Effect::Publish { .. }), "{:?}", rig.effect);
    }

    #[test]
    fn with_nothing_to_tell_it_is_only_an_hour_at_the_computer() {
        let mut rig = office();
        let mut task = Task::write_news(Point::new(4, 4), 0.2);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert_eq!(rig.effect, Effect::None);
    }

    #[test]
    fn a_dark_computer_writes_nothing() {
        let mut rig = office();
        let dark = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.facilities.features = crate::sim::feature::Features::from_map(&rig.world.map, &dark);
        assert_eq!(rig.tick(&mut Task::write_news(Point::new(4, 4), 0.2)), TaskResult::Failed);
    }
}
