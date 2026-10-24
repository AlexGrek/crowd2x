//! The accountant reads and closes one finished day's books at its computer.

use crate::map::Point;
use crate::sim::biology::Biology;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::economy::Money;
use crate::sim::feature::FeatureKind;
use crate::sim::Effect;
use super::super::action::Action;
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use super::Work;

pub const BOOKS_SECONDS: f32 = watched(30.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CloseBooks {
    pub desk: Point,
    pub seconds: f32,
}

impl TaskExecutor for CloseBooks {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let result = Work { desk: self.desk, seconds: self.seconds, place: FeatureKind::AccountantComputer }.execute(ctx);
        if result != TaskResult::Success { return result; }
        let economy = ctx.think.economy;
        let Some(day) = economy.unclosed_day(ctx.think.clock.day()) else { return result; };
        let book = economy.book(day).expect("unclosed day has a book");
        let week: Money = economy.books().iter().filter(|book| book.day <= day && book.day.saturating_add(7) > day).map(|book| book.balance()).sum();
        let runway = if week.is_negative() {
            let days = (economy.budget().as_cents() as i128 * 7 / -(week.as_cents() as i128)).max(0);
            format!(", enough for {days} days")
        } else { String::new() };
        ctx.think.log.important(format!(
            "the accountant closed the books for day {day} ({}): budget {}; the day {}; the week {}{runway}",
            book.weekday().short(), economy.budget(), book.balance().signed(), week.signed()
        ));
        let uid = ctx.walk.body().uid();
        ctx.think.log.unit(uid, format!(
            "{uid} closed day {day}: wages {}, taxes {}, care {}, sales {}, estates {}, grants {}",
            book.wages, book.taxes, book.care, book.sales, book.estates, book.grants
        ));
        *ctx.effect = Effect::CloseBooks { day };
        result
    }

    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        Work { desk: self.desk, seconds: self.seconds, place: FeatureKind::AccountantComputer }.interrupted(action, biology);
    }

    fn describe(&self) -> String { format!("close the books at {}, {}", self.desk.x, self.desk.y) }
}

impl Task {
    pub const fn close_books(desk: Point, seconds: f32) -> Task { Task::CloseBooks(CloseBooks { desk, seconds }) }
}

impl TimedTask for CloseBooks {
    const NAME: &'static str = "close the books";
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
    use crate::sim::clock::{Clock, DAY, TIME_SCALE};

    #[test]
    fn books_are_reported_and_closed_only_after_work_at_a_powered_computer() {
        let desk = Point::new(4, 4);
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        prop_at(&mut map, "accountant computer", desk);
        let mut rig = Rig::new(map, Point::new(3, 4));
        rig.world.facilities.economy.roll(1);
        rig.world.facilities.economy.roll(2);
        rig.world.clock = Clock::after_watching(DAY as f64 / TIME_SCALE as f64);
        let mut task = Task::close_books(desk, 0.2);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert_eq!(rig.effect, Effect::CloseBooks { day: 1 });
        assert!(rig.world.log.drain().iter().any(|line| line.contains("closed the books for day 1")));
        rig.action = Action::None;
        let supply = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.features = crate::sim::feature::Features::from_map(&rig.world.map, &supply);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.effect, Effect::None);
    }
}
