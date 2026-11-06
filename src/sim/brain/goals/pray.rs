//! [`PrayGoal`]: walk to the nearest pew in the church and pray there.
//!
//! **When** is the routine's (`StayBusyRoutine`, [`wants_to_pray`]): now and
//! then while wandering, as often as the unit's religiosity says
//! ([`crate::sim::religion::prayer_gap`]). This goal is the visit: the nearest
//! pew it may reach ([`Seek`], so a crowd at one spreads over the others),
//! the walk beside it, [`Task::pray`] for
//! [`PRAYER_SECONDS`], and remembering when it prayed ([`PRAYED`]), so the
//! next is a while off. What the prayer does to the mind is the task's and
//! the body's.
//!
//! [`wants_to_pray`]: super::super::routines::wants_to_pray

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::religion::PRAYER_SECONDS;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::memory::{Recall, PRAYED};
use super::super::task::{Task, TaskResult};
use super::{choose_nearest, nearest_in_reach, stand_beside, Seek, Stand};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PrayGoal {
    /// Which pew it is going to, and how the way there has gone.
    seek: Seek,
}

impl PrayGoal {
    pub fn new() -> PrayGoal {
        PrayGoal::default()
    }

    /// Queue the rest **from what is true now**: walk beside the pew (unless
    /// already there), pray. `false` with nowhere to stand beside it.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(pew) = self.seek.target else {
            return false;
        };
        match stand_beside(ctx, pew) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::pray(pew, PRAYER_SECONDS));
        true
    }

    /// Prayed: when, for the routine to leave the next a while, and a line in
    /// the log.
    fn prayed(&mut self, ctx: &mut GoalCtx<'_>, pew: Point) {
        let now = ctx.think.clock.elapsed() as u64;
        ctx.memory.remember(PRAYED, Recall::Time(now));
        let uid = ctx.body.uid();
        let service = if ctx.think.services.is_on(pew) { ", at a service" } else { "" };
        ctx.think.log.unit(uid, format!("{uid} prayed at the pew at {}, {}{service}", pew.x, pew.y));
        self.seek.forget();
    }
}

impl GoalExecutor for PrayGoal {
    fn goal(&self) -> GoalId {
        GoalId::Pray
    }

    fn save(&self) -> pb::Executor {
        let PrayGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// The half-walked route is gone, the pew is not.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.seek.target.is_none() {
            return;
        }
        self.seek.steady();
        if !self.plan(ctx) {
            self.seek.forget();
        }
    }

    /// A prayer finished on the tick something else took over is still a
    /// prayer said: remember it here, or the routine would send it straight
    /// back to the church.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() == TaskResult::Success
            && let Some(Task::Pray(task)) = ctx.finished
        {
            self.prayed(ctx, task.pew);
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() {
            return GoalProgress::Blocked;
        }
        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_)) | Some(Task::Pray(_))) => {
                // A body in the way, or no way there: wait, or the next pew.
                ctx.tasks.clear();
                if self.seek.setback(ctx, |ctx| nearest_in_reach(ctx, FeatureKind::Pew)) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                ctx.tasks.clear();
                self.seek.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, Some(Task::Pray(task))) => {
                self.prayed(ctx, task.pew);
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        let Some(pew) = choose_nearest(ctx, FeatureKind::Pew) else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(pew);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.seek.forget();
            GoalProgress::Blocked
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::map::{Map, Point, Size, FLOOR, WALL};
    use crate::sim::biology::ProcessId;
    use crate::sim::brain::goal::GoalId;
    use crate::sim::brain::routines::PRAYING;
    use crate::sim::clock::{Clock, HOUR, TIME_SCALE};
    use crate::sim::kinds::Human;
    use crate::sim::religion::{prayer_gap, PRAYER_SLOT};
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::walker::ROUTES_ASKED;
    use crate::sim::GameEntity;

    const PRAYED: &str = "prayed at the pew at";
    const PEW: Point = Point::new(6, 2);

    /// A church of a room: a pew, room round it.
    fn church() -> Map {
        let mut map = Map::new(Size::new(13, 9), FLOOR);
        prop_at(&mut map, "pew", PEW);
        map
    }

    /// A wanderer of `religiosity`, its mind at 50 and every need but fun off.
    fn believer(religiosity: i32) -> Human {
        let mut human = needy_human(Point::new(6, 6), 0.0, 0.0, 0.0);
        human.body_mut().set_religiosity(religiosity);
        human.biology_mut().unwrap().edit(|stats| stats.with_mental_health(50.0));
        // Fed, awake and comfortable for days on end, in a church with no
        // fridge or bed: a test about faith, not about a body's needs.
        for process in [ProcessId::Energy, ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder] {
            human.biology_mut().unwrap().set_running(process, false);
        }
        human
    }

    /// Put the world's clock at world second `at`, by day.
    fn at(world: &mut World, at: f64) {
        world.clock = Clock::after_watching(at / TIME_SCALE as f64);
    }

    /// Step `human` slot after slot from ten in the morning, its needs held
    /// still, for `slots` slots of `ticks` ticks each; how many prayers the
    /// log has by then.
    fn days_of(world: &mut World, human: &mut Human, from: f64, slots: u32, ticks: u32) -> usize {
        for slot in 0..slots {
            at(world, from + slot as f64 * PRAYER_SLOT + 1.0);
            for _ in 0..ticks {
                world.step(human);
                human.set_fun(100.0);
            }
        }
        world.lines().iter().filter(|line| line.contains(PRAYED)).count()
    }

    /// Somebody devout wandering by day walks to the pew, prays, and comes
    /// away with its mind mended; the log says so.
    #[test]
    fn a_devout_wanderer_walks_to_the_pew_and_prays_there() {
        let mut world = World::new(church());
        let mut human = believer(100);
        let prayers = days_of(&mut world, &mut human, 10.0 * HOUR as f64, 48, 300);
        assert!(prayers >= 1, "never prayed in a day: {:?}", world.lines().iter().rev().take(10).collect::<Vec<_>>());
        assert!(human.stats().mental_health() > 50.0, "{}", human.stats().mental_health());
    }

    /// The faithless never go, however long they wander.
    #[test]
    fn somebody_with_no_faith_never_prays() {
        let mut world = World::new(church());
        let mut human = believer(5);
        assert_eq!(days_of(&mut world, &mut human, 10.0 * HOUR as f64, 96, 60), 0);
    }

    /// One prayer is not followed by the next before its gap is out: the
    /// devout pray twice a day at most, somebody half as devout once.
    #[test]
    fn a_prayer_is_not_followed_by_another_before_its_religiosity_says() {
        for religiosity in [100, 50] {
            let mut world = World::new(church());
            let mut human = believer(religiosity);
            let slots = (2.0 * 24.0 * HOUR as f64 / PRAYER_SLOT) as u32;
            let prayers = days_of(&mut world, &mut human, 0.0, slots, 300);
            let most = (2.0 * 24.0 * HOUR as f64 / prayer_gap(religiosity as u8)).ceil() as usize + 1;
            assert!((1..=most).contains(&prayers), "{religiosity}: {prayers} prayers in two days, at most {most}");
        }
    }

    /// Off means off: with no mind to mend, there is nothing to pray for.
    #[test]
    fn nobody_with_the_mind_switched_off_prays() {
        let mut world = World::new(church());
        let mut human = believer(100);
        human.biology_mut().unwrap().set_running(ProcessId::Mind, false);
        assert_eq!(days_of(&mut world, &mut human, 10.0 * HOUR as f64, 96, 60), 0);
    }

    /// A pew walled off is given up on, not walked at every tick: the goal is
    /// held off, and few routes are asked for it.
    #[test]
    fn a_pew_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        let mut map = church();
        for x in 0..13 {
            map.set_terrain(Point::new(x, 4), WALL);
        }
        let mut world = World::new(map);
        let mut human = believer(100);
        human.brain_mut().memory_mut().forget(crate::sim::brain::memory::PRAYED);
        ROUTES_ASKED.with(|asked| asked.set(0));
        let mut wanted = false;
        for slot in 0..48 {
            at(&mut world, 10.0 * HOUR as f64 + slot as f64 * PRAYER_SLOT + 1.0);
            for _ in 0..100 {
                world.step(&mut human);
                human.set_fun(100.0);
                wanted |= human.brain().goals().priority(GoalId::Pray) == PRAYING;
            }
        }
        assert!(wanted, "it did want to pray");
        assert!(!world.log_contains(PRAYED));
        let asked = ROUTES_ASKED.with(|asked| asked.get());
        assert!(asked < 200, "{asked} routes in 4800 ticks");
    }
}
