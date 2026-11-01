//! [`PhoneGoal`]: stop where one stands and look at the phone.
//!
//! **When** is the routine's (`StayBusyRoutine`, the walk about the place
//! that a unit with nothing better to do is on): from time to time, while
//! wandering, and never with fun over [`PHONE_CEILING`](crate::sim::biology::fun::PHONE_CEILING). This goal is only the
//! look — [`Task::phone`] for [`PHONE_SECONDS`] — and remembering when it was,
//! so that the next is a while off.

use crate::save::{invalid, pb, Invalid};

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::memory::{Recall, PHONED};
use super::super::task::{Task, TaskResult};
use super::super::tasks::phone::PHONE_SECONDS;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PhoneGoal;

impl PhoneGoal {
    pub fn new() -> PhoneGoal {
        PhoneGoal
    }
}

impl GoalExecutor for PhoneGoal {
    fn goal(&self) -> GoalId {
        GoalId::Phone
    }

    fn save(&self) -> pb::Executor {
        pb::Executor::Wander(pb::Wander { target: None, retries: 0 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        match saved {
            pb::Executor::Wander(_) => Ok(()),
            _ => invalid("phone is a wander"),
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if matches!(ctx.finished, Some(Task::Phone(_))) {
            match last {
                // Seen to: when, for the routine to leave it a while.
                TaskResult::Success => {
                    let now = ctx.think.clock.elapsed() as u64;
                    ctx.memory.remember(PHONED, Recall::Time(now));
                    let uid = ctx.body.uid();
                    ctx.think.log.unit(uid, format!("{uid} looked at its phone"));
                    return GoalProgress::Achieved;
                }
                TaskResult::Failed => {
                    ctx.tasks.clear();
                    return GoalProgress::Blocked;
                }
                _ => {}
            }
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        let _ = ctx.tasks.push_back(Task::phone(ctx.body.center_position(), PHONE_SECONDS));
        GoalProgress::Working
    }
}

#[cfg(test)]
mod tests {
    use crate::map::{Map, Point, Size, FLOOR};
    use crate::sim::biology::fun::{FUN_PER_SECOND, PHONE_CEILING, PHONE_FUN};
    use crate::sim::brain::routines::{PHONE_GAP, PHONE_SLOT};
    use crate::sim::clock::{Clock, TIME_SCALE};
    use crate::sim::testing::{needy_human, World};

    const LOOKED: &str = "looked at its phone";

    /// Put the world's clock at world second `at`.
    fn at(world: &mut World, at: f64) {
        world.clock = Clock::after_watching(at / TIME_SCALE as f64);
    }

    /// A wanderer with `fun`, stepped through slot after slot of the day
    /// until it phones — or `slots` of them have gone by, its fun held where
    /// it began if `hold`. The clock stands still within a slot, as
    /// `World::step` leaves it.
    fn wander_for(fun: f32, slots: u32, hold: bool) -> (World, crate::sim::kinds::Human, Option<u32>) {
        let mut world = World::new(Map::new(Size::new(11, 11), FLOOR));
        let mut human = needy_human(Point::new(5, 5), 0.0, 0.0, 0.0);
        human.set_fun(fun);
        for slot in 0..slots {
            at(&mut world, slot as f64 * PHONE_SLOT + 1.0);
            for _ in 0..40 {
                world.step(&mut human);
                if hold {
                    human.set_fun(fun);
                }
            }
            if world.log_contains(LOOKED) {
                return (world, human, Some(slot));
            }
        }
        (world, human, None)
    }

    #[test]
    fn a_wanderer_now_and_then_stops_and_looks_at_its_phone_for_a_little_fun() {
        let (_, human, found) = wander_for(50.0, 400, false);
        let slot = found.expect("never took out its phone in 400 slots");
        // What boredom drained meanwhile is all it is short of the look's worth.
        let drained = (slot + 3) as f32 * PHONE_SLOT as f32 * FUN_PER_SECOND;
        assert!(human.stats().fun() > 50.0 + PHONE_FUN - drained, "{}", human.stats().fun());
    }

    #[test]
    fn nobody_over_the_ceiling_takes_out_a_phone() {
        let (_, _, found) = wander_for(PHONE_CEILING + 1.0, 400, true);
        assert_eq!(found, None);
        let (_, _, found) = wander_for(PHONE_CEILING, 400, true);
        assert!(found.is_some(), "at the ceiling is still room for more fun");
    }

    #[test]
    fn a_look_is_not_followed_by_another_within_the_gap() {
        let (mut world, mut human, found) = wander_for(30.0, 400, true);
        let slot = found.unwrap();
        let now = slot as f64 * PHONE_SLOT + 1.0;
        let looks = |world: &World| world.lines().iter().filter(|line| line.contains(LOOKED)).count();
        assert_eq!(looks(&world), 1);
        for later in 1..(PHONE_GAP / PHONE_SLOT) as u32 {
            at(&mut world, now + later as f64 * PHONE_SLOT);
            human.set_fun(30.0);
            for _ in 0..300 {
                world.step(&mut human);
                human.set_fun(30.0);
            }
        }
        assert_eq!(looks(&world), 1, "the next is a while off");
    }

    #[test]
    fn nobody_with_fun_switched_off_takes_out_a_phone() {
        use crate::sim::biology::ProcessId;
        use crate::sim::GameEntity;
        let mut world = World::new(Map::new(Size::new(11, 11), FLOOR));
        let mut human = needy_human(Point::new(5, 5), 0.0, 0.0, 0.0);
        human.set_fun(20.0);
        human.biology_mut().unwrap().set_running(ProcessId::Fun, false);
        for slot in 0..400 {
            at(&mut world, slot as f64 * PHONE_SLOT + 1.0);
            for _ in 0..40 {
                world.step(&mut human);
            }
        }
        assert!(!world.log_contains(LOOKED));
    }
}
