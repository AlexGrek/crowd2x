//! [`Book`]: somebody about to sleep makes the bed of an outpost its own
//! (`sim::outposts`): it stays theirs, so they can come back to it whenever it
//! is the nearest bed they have.
//!
//! No action, nothing walked to: the bookings are the world's, so the task
//! *asks* ([`Effect::Book`]) the tick it starts, and the world step hands the
//! bed out in slot order. The goal that queued it walks to the bed and sleeps
//! in it either way; who got it is read next time a bed is chosen.

use crate::map::Point;
use crate::sim::Effect;

use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Book {
    /// The bed's cell.
    pub bed: Point,
}

impl TaskExecutor for Book {
    /// Asked once, at once: fails — and asks nothing — where the bed is not a
    /// built outpost bed, is somebody's already, or the sleeper has one booked.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let uid = ctx.walk.body().uid();
        if !ctx.think.outposts.can_book(self.bed, uid) {
            return TaskResult::Failed;
        }
        *ctx.effect = Effect::Book { bed: self.bed };
        TaskResult::Success
    }

    fn describe(&self) -> String {
        format!("book the outpost bed at {}, {}", self.bed.x, self.bed.y)
    }
}

impl Task {
    /// Book the outpost bed at `bed`.
    pub const fn book(bed: Point) -> Task {
        Task::Book(Book { bed })
    }
}

impl TimedTask for Book {
    const NAME: &'static str = "book a bed";

    fn new(cell: Point, _seconds: f32) -> Self {
        Book { bed: cell }
    }

    fn cell(&self) -> Point {
        self.bed
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
    use crate::sim::outposts::built_outpost_with_beds;

    #[test]
    fn somebody_asks_for_a_free_outpost_bed_once_and_for_no_other_cell() {
        let map = Map::new(Size::new(12, 10), FLOOR).with_wasteland(3);
        let mut rig = Rig::new(map, Point::new(2, 2));
        let beds = built_outpost_with_beds(&mut rig.world.facilities.outposts, &mut rig.world.map);
        let uid = rig.walk.body().uid();

        let mut task = Task::book(beds[0]);
        assert_eq!(rig.tick(&mut task), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Book { bed: beds[0] });
        assert_eq!(rig.tick(&mut Task::book(Point::new(2, 2))), TaskResult::Failed, "a floor is no bed to book");

        assert!(rig.world.facilities.outposts.book(beds[0], uid));
        assert_eq!(rig.tick(&mut Task::book(beds[1])), TaskResult::Failed, "one bed to a person");
        assert_eq!(rig.effect, Effect::None);
    }
}
