//! [`BuryGoal`]: an undertaker's watch — to the nearest of the dead, take the
//! body up, carry it back and see it into its own crematorium.
//!
//! One body is the goal's unit of work, as a packful is `CollectGoal`'s: it
//! reports `Achieved` when a body is cremated, and while the routine still
//! wants it (`WorkRoutine`: the watch is due, or a body is in hand) the next
//! call starts from wherever the undertaker stands.
//!
//! **It plans from what is true now** (rule 4) and never counts:
//!
//! * **a body in hand** — its own crematorium (the job's workplace): beside
//!   it, and [`Cremate`](crate::sim::brain::tasks::Cremate);
//! * **something else in hand** — finished first (rule 15), so the hand is
//!   free to take a body;
//! * **a free hand** — the nearest body it could get to ([`nearest_body`]):
//!   beside it, and [`TakeBody`](crate::sim::brain::tasks::TakeBody). The
//!   body lands in the hand at the end of the tick the work finished in (the
//!   world step's doing), so the tick after, the hand says what next;
//! * **nobody to see to** — `Blocked`, and the goal's back-off spaces out the
//!   next look.
//!
//! A body it cannot get to is given up on for the next nearest
//! ([`Seek::setback`]); a body taken by another undertaker under it is a
//! cheap replan. **A body inside a locked house is out of reach**: an
//! undertaker holds the key to its own home and no other, so the dead of a
//! house lie there until something else changes.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::brain::tasks::{CREMATE_SECONDS, TAKE_BODY_SECONDS};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Seek, Stand};

/// The body nearest this unit, as the crow flies, that it could get to: one
/// lying in a cell it may stand in or beside that the map's components say
/// it can reach, and that it does not remember as out of reach. Ties go to
/// the lower cell. A walk of the dead ([`Think::corpses`]), never the crowd.
///
/// [`Think::corpses`]: crate::sim::Think::corpses
pub(crate) fn nearest_body(ctx: &GoalCtx<'_>) -> Option<Point> {
    let here = ctx.body.center_position();
    let now = ctx.think.clock.elapsed();
    let places = ctx.memory.out_of_reach();
    let key = ctx.body.home();
    let navigation = ctx.think.map.navigation();
    let mut best: Option<(i32, Point)> = None;
    for (_, cell) in ctx.think.corpses.iter() {
        let candidate = (here.manhattan_distance(cell) as i32, cell);
        if best.is_some_and(|best| best <= candidate) || places.contains(cell, now) {
            continue;
        }
        let reachable = std::iter::once(cell)
            .chain(cell.cardinal_neighbours())
            .any(|side| ctx.think.is_passable_with(side, key) && navigation.may_reach(here, side));
        if reachable {
            best = Some(candidate);
        }
    }
    best.map(|(_, cell)| cell)
}

/// See the dead to the crematorium for a living.
///
/// What it remembers is where it is going — a body, or its crematorium — and
/// how the way there has gone. Which crematorium is its own is the job's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BuryGoal {
    seek: Seek,
}

impl BuryGoal {
    pub fn new() -> BuryGoal {
        BuryGoal::default()
    }

    /// The undertaker's own crematorium, if it is there to be used: the job's
    /// workplace, and a crematorium there.
    fn crematorium(ctx: &GoalCtx<'_>) -> Option<Point> {
        let at = ctx.job?.workplace;
        ctx.think.features.has(FeatureKind::Crematorium, at).then_some(at)
    }

    /// Queue the walk beside the target (unless already there) and `then`.
    /// `false` when nowhere beside it can be stood on.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>, then: Task) -> bool {
        let Some(target) = self.seek.target else {
            return false;
        };
        match stand_beside(ctx, target) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(then);
        true
    }

    /// The work to do once beside the target.
    fn work_at(&self, ctx: &GoalCtx<'_>) -> Option<Task> {
        let target = self.seek.target?;
        Some(if Some(target) == ctx.job.map(|job| job.workplace) {
            Task::cremate(target, CREMATE_SECONDS)
        } else {
            Task::take_body(target, TAKE_BODY_SECONDS)
        })
    }

    fn head_for(&mut self, place: Point) {
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
    }

    fn forget(&mut self) {
        self.seek.forget();
    }
}

impl GoalExecutor for BuryGoal {
    fn goal(&self) -> GoalId {
        GoalId::Bury
    }

    fn save(&self) -> pb::Executor {
        let BuryGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: a target no longer worth going to — a body somebody
    /// else took, a crematorium that is not the job's any more — is
    /// forgotten; [`BuryGoal::process`] plans the rest from where it stands.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        let worth_it = |target: Point| match ctx.job {
            Some(job) if job.workplace == target => Self::crematorium(ctx).is_some(),
            _ => ctx.think.corpses.at(target).is_some(),
        };
        match self.seek.target {
            Some(target) if worth_it(target) => self.seek.steady(),
            Some(_) => self.forget(),
            None => {}
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(job), Some(inventory)) = (ctx.biology, ctx.job, ctx.inventory) else {
            return GoalProgress::Blocked;
        };
        let crematorium = job.workplace;
        let hand = inventory.hand();

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // next nearest body — but no other crematorium is this one's.
                let retry = if self.seek.target == Some(crematorium) {
                    self.seek.setback(ctx, |_| None)
                } else {
                    self.seek.setback(ctx, nearest_body)
                };
                if retry && let Some(then) = self.work_at(ctx) && self.plan(ctx, then) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            // Taken by somebody else, the hand filled, pulled away: worked
            // out again below, from what is true now. Cheap.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Cremate(_))) => {
                self.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        match hand {
            Some(ItemKind::Body) => {
                let Some(crematorium) = Self::crematorium(ctx) else {
                    self.forget();
                    return GoalProgress::Blocked;
                };
                self.head_for(crematorium);
                if self.plan(ctx, Task::cremate(crematorium, CREMATE_SECONDS)) {
                    GoalProgress::Working
                } else {
                    self.forget();
                    GoalProgress::Blocked
                }
            }
            Some(other) => {
                // Rule 15: a hand full of something else is emptied first.
                let _ = ctx.tasks.push_back(Task::consume(other, other.consume_seconds()));
                GoalProgress::Working
            }
            None => {
                let Some(body) = nearest_body(ctx) else {
                    self.forget();
                    return GoalProgress::Blocked;
                };
                self.head_for(body);
                if self.plan(ctx, Task::take_body(body, TAKE_BODY_SECONDS)) {
                    GoalProgress::Working
                } else {
                    self.forget();
                    GoalProgress::Blocked
                }
            }
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "going to",
            match self.seek.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "none".to_string(),
            },
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::biology::ProcessId;
    use crate::sim::clock::{Clock, DAY, HOUR, TIME_SCALE};
    use crate::sim::job::{Job, JobKind, WATCHES};
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::{GameEntity, Human};

    const CREMATORIUM: Point = Point::new(2, 2);
    const BODY: Point = Point::new(14, 8);

    /// `hours` o'clock on day `day` — day 1 is a Monday.
    fn at(day: u32, hours: f32) -> Clock {
        let world = (day - 1) as f64 * DAY as f64 + (hours as f64 - 8.0) * HOUR as f64;
        Clock::after_watching(world / TIME_SCALE as f64)
    }

    fn ticks(world: &World, hours: f32) -> u32 {
        (hours * HOUR / TIME_SCALE / world.dt) as u32
    }

    /// An undertaker on the day watch beside its crematorium in an open
    /// room, its needs switched off, and a body across the room.
    fn morgue(body: Option<Point>) -> (World, Human) {
        let mut map = Map::new(Size::new(18, 12), FLOOR);
        prop_at(&mut map, "crematorium", CREMATORIUM);
        let mut world = World::new(map);
        world.clock = at(1, 10.0);
        if let Some(body) = body {
            world.corpses.add(Uid::new(EntityType::Human, 77), body);
        }
        let mut human = needy_human(Point::new(3, 2), 0.0, 0.0, 0.0);
        for process in ProcessId::ALL {
            human.biology_mut().unwrap().set_running(process, false);
        }
        assert!(human.hire(Job::new(JobKind::Undertaker, CREMATORIUM, WATCHES[0])));
        (world, human)
    }

    #[test]
    fn an_undertaker_on_watch_takes_up_the_body_and_cremates_it() {
        let (mut world, mut human) = morgue(Some(BODY));
        let mut took = false;
        for _ in 0..ticks(&world, 3.0) {
            world.step(&mut human);
            took |= human.carried() == Some(ItemKind::Body);
            if world.log_contains("cremated a body") {
                break;
            }
        }
        assert!(took, "it carried the body");
        assert!(world.log_contains("took up the body of"), "{:?}", world.lines());
        assert!(world.log_contains("cremated a body in the crematorium at 2, 2"), "{:?}", world.lines());
        assert!(world.corpses.is_empty());
        assert_eq!(world.burials.len(), 1, "and the body leaves the world");
        assert_eq!(human.carried(), None);
    }

    #[test]
    fn with_nobody_dead_an_undertaker_is_held_off_rather_than_searching() {
        let (mut world, mut human) = morgue(None);
        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Bury);
        assert!(human.brain().goals().cooldown(GoalId::Bury) > 0, "held off");
    }

    /// The watch ends with a body in hand: it is still seen to the fire.
    #[test]
    fn a_body_in_hand_is_cremated_after_the_watch_is_over() {
        let (mut world, mut human) = morgue(None);
        world.clock = at(1, 20.0);
        human.set_carried(Some(ItemKind::Body));
        for _ in 0..ticks(&world, 1.0) {
            world.step(&mut human);
            if world.log_contains("cremated a body") {
                break;
            }
        }
        assert!(world.log_contains("cremated a body"), "{:?}", world.lines());
    }

    /// The day watch is not the night's: nobody goes out for the dead off
    /// their watch, with an empty hand.
    #[test]
    fn off_watch_nobody_goes_out_for_the_dead() {
        let (mut world, mut human) = morgue(Some(BODY));
        world.clock = at(1, 20.0);
        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Bury);
        assert!(!world.corpses.is_empty());
    }

    /// A hungry undertaker with a body does not eat it out of its hand.
    #[test]
    fn nobody_clears_a_hand_with_a_body_in_it_for_a_meal() {
        let (mut world, mut human) = morgue(None);
        prop_at(&mut world.map, "fridge", Point::new(16, 10));
        world.features = crate::sim::Features::from_map(&world.map, &crate::map::utilities::Supply::everywhere(world.map.size()));
        human.biology_mut().unwrap().set_running(ProcessId::Hunger, true);
        human.set_hunger(95.0);
        human.set_carried(Some(ItemKind::Body));
        let mut cremated = false;
        for _ in 0..ticks(&world, 1.0) {
            world.step(&mut human);
            assert!(
                !matches!(human.brain().current_task(), Some(Task::ConsumeItem(task)) if task.item == ItemKind::Body),
                "it would eat the body"
            );
            cremated |= world.log_contains("cremated a body");
        }
        assert!(cremated, "{:?}", world.lines());
    }
}
