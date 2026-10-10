//! [`SleepGoal`]: walk to bed, get into it, and sleep.
//!
//! The same plan as [`RelieveGoal`](super::RelieveGoal) — a bed, like a toilet,
//! is used by entering it — with two differences.
//!
//! **What an ending means.** The [`Sleep`](crate::sim::brain::tasks::Sleep) task
//! is *one hour of world* in bed, and a night is many of them: this goal
//! reports `Achieved` after each, and if the routine still wants sleep it stays
//! on top and starts the next from where it lies. **How many hours happen is
//! the routine's to say** (`SleepRoutine`: rested and daylight), and this goal
//! never has to know. That is also what makes a night interruptible at a cost
//! of an hour: between two stretches somebody with a full bladder outranks the
//! bed, gets up, and comes back.
//!
//! **Which bed.** A human has a bed of its own, handed out at spawn and
//! remembered ([`HOME_BED`]) — and by default that is the one it goes to, and
//! the only one:
//!
//! 1. **Already lying in a bed**: that one, whoever's it is. Getting up to walk
//!    home across the room half way through the night would throw the hour
//!    away.
//! 2. **Its own bed, if it is free.**
//! 3. **Any other free bed, chosen at random — but only when critically
//!    tired** ([`CRITICALLY_TIRED`]). Somebody who merely wants to sleep does
//!    not take a bed that is not theirs; somebody who cannot go on will.
//! 4. Otherwise **nothing**: the goal is held off like any other that found
//!    itself impossible, and the unit stays up.
//!
//! A bed it could not get to, or into, is remembered as out of reach for a
//! while ([`OutOfReach`](crate::sim::brain::memory::OutOfReach)) and passed
//! over in 2 and 3 for any bed that is not — so one critically tired tries
//! another bed rather than the same jammed door. Straight after giving up that
//! is final; at the start of the next stretch a remembered bed is still taken
//! if nothing else will do, so nobody is kept from their own bed by a memory.

use crate::map::Point;
use crate::sim::clock::{watched, HOUR};
use crate::sim::feature::FeatureKind;
use crate::sim::rng::tick_rng;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::memory::{Recall, HOME_BED};
use super::super::routines::CRITICALLY_TIRED;
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Seek, Stand};

/// One stretch of sleep: an hour of world in bed, thirty seconds of watching
/// it. Written in world units like every duration a body is watched standing
/// through (see [`crate::sim::clock`]).
pub const SLEEP_SECONDS: f32 = watched(HOUR);

/// Sleep in a bed.
///
/// What it remembers is the bed it chose for the stretch in hand and how many
/// times in a row getting to it, or into it, has been refused by a body. Which
/// bed is *its own* is not here: that is the unit's memory, and outlives the
/// goal being put down.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SleepGoal {
    /// Which place it is going to, and how the way there has gone.
    seek: Seek,
}

impl SleepGoal {
    pub fn new() -> SleepGoal {
        SleepGoal::default()
    }

    /// Which bed the next stretch is in, or `None` when there is not one this
    /// unit will take — see the module docs for the order. Reads the crowd
    /// for the cells of beds and nothing else.
    ///
    /// Beds it gave up on getting to lately are passed over. When `rerouting`
    /// — straight after giving up on one — that is the end of it; at the start
    /// of a stretch, a remembered bed is still taken if no other will do, the
    /// way [`super::choose_nearest`] treats a lone toilet.
    fn choose(ctx: &GoalCtx<'_>, rerouting: bool) -> Option<Point> {
        let think = ctx.think;
        let here = ctx.body.center_position();
        let uid = ctx.body.uid();

        // Already in one: it is the bed for the next stretch.
        if think.features.has(FeatureKind::Bed, here) {
            return Some(here);
        }

        let now = think.clock.elapsed();
        let places = ctx.memory.out_of_reach();
        let home = match ctx.memory.get(HOME_BED) {
            Some(&Recall::Cell(home)) => Some(home),
            _ => None,
        };
        let critical = ctx.biology.is_some_and(|biology| biology.stats().tiredness() >= CRITICALLY_TIRED);
        let pick = |remembered_too: bool| {
            let free = |bed: Point| think.occupancy.is_free_for(bed, uid) && (remembered_too || !places.contains(bed, now));
            // Its own, by default.
            if let Some(home) = home
                && free(home)
            {
                return Some(home);
            }
            // Anybody's, only when it cannot go on.
            if !critical {
                return None;
            }
            // ...and never one in somebody else's house.
            let key = ctx.body.home();
            let mut rng = tick_rng(uid, think.tick);
            think
                .features
                .pick_where(FeatureKind::Bed, &mut rng, |bed| think.may_use(bed, key) && free(bed))
        };
        pick(false).or_else(|| if rerouting { None } else { pick(true) })
    }

    /// Queue the rest **from what is true now**, on the back of the queue:
    /// walk beside the bed (unless already there or in it), sleep in it.
    /// `false` when there is nowhere to stand to get in.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(bed) = self.seek.target else {
            return false;
        };
        match stand_beside(ctx, bed) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_to(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::sleep(bed, SLEEP_SECONDS));
        true
    }

    fn forget(&mut self) {
        self.seek.forget();
    }

    /// One stretch done: say so, and start the next from scratch.
    fn slept(&mut self, ctx: &GoalCtx<'_>, bed: Point) {
        ctx.think
            .log
            .push(format!("{} slept in the bed at {}, {}", ctx.body.uid(), bed.x, bed.y));
        self.forget();
    }
}

impl GoalExecutor for SleepGoal {
    fn goal(&self) -> GoalId {
        GoalId::Sleep
    }

    /// The half-walked route is gone, the bed is not.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.seek.target.is_none() {
            return;
        }
        self.seek.steady();
        if !self.plan(ctx) {
            self.forget();
        }
    }

    /// A stretch of sleep that ended the very tick something else took over —
    /// somebody rested enough to be released by the routine — is heard here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() == TaskResult::Success
            && let Some(Task::Sleep(task)) = ctx.finished
        {
            self.slept(ctx, task.bed);
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() {
            // A kind with nothing to rest.
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_)) | Some(Task::Sleep(_))) => {
                // A body in the way of the walk there, or already in the bed
                // when this one tried to get in — something a moving body
                // caused, so it may move again.
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there, or
                // no end to the crowd: remember it as out of reach and try
                // the next one (see `Seek`).
                if self.seek.setback(ctx, |ctx| SleepGoal::choose(ctx, true)) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                // Nowhere left to try: give up on this visit rather than
                // retrying every tick.
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Standing in the wrong place. Cheap to work out again next
                // tick.
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, Some(Task::Sleep(task))) => {
                self.slept(ctx, task.bed);
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start again.
        let Some(bed) = SleepGoal::choose(ctx, false) else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(bed);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "bed",
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
    use crate::map::{Map, Size, FLOOR, WALL};
    use crate::sim::biology::ProcessId;
    use crate::sim::brain::routines::{RESTED, SLEEPY};
    use crate::sim::brain::GoalId;
    use crate::sim::clock::{Clock, TIME_SCALE};
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::{GameEntity, Human};

    /// Half past midnight, when sleepiness is enough to go to bed.
    fn midnight() -> Clock {
        Clock::after_watching((16.5 * HOUR / TIME_SCALE) as f64)
    }

    /// Nothing but tiredness pressing: the other needs are off the table.
    fn tired_human(cell: Point, tiredness: f32) -> Human {
        let mut human = needy_human(cell, 0.0, 0.0, 0.0);
        human.set_stamina(100.0 - tiredness);
        human
    }

    /// Tired, and with every other process switched off, so a test about a
    /// night is about a night and not a race between five needs on a map that
    /// has nothing to satisfy four of them.
    fn only_tired(cell: Point, tiredness: f32) -> Human {
        let mut human = tired_human(cell, tiredness);
        let biology = human.biology_mut().unwrap();
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
            biology.set_running(process, false);
        }
        human
    }

    /// The same, with `bed` as its own — what the spawn pass does for a unit
    /// that arrives to find one free.
    fn owner(cell: Point, tiredness: f32, bed: Point) -> Human {
        let mut human = only_tired(cell, tiredness);
        assert!(human.set_home(bed, None));
        human
    }

    /// A tired human at night, in a world with these beds.
    fn night_with_beds(beds: &[Point]) -> World {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        for (i, &bed) in beds.iter().enumerate() {
            prop_at(&mut map, &format!("bed {}", 1 + i % 6), bed);
        }
        let mut world = World::new(map);
        world.clock = midnight();
        world
    }

    fn run_until_slept(world: &mut World, human: &mut Human, ticks: u32) {
        for _ in 0..ticks {
            world.step(human);
            if world.sleeps() > 0 {
                break;
            }
        }
    }

    /// Eyes shut: whoever stands round a bed, its sleeper meets nobody — and
    /// is not glad of company in the middle of the night.
    #[test]
    fn a_sleeper_sees_nobody_and_meets_nobody() {
        use crate::sim::background::Priority;
        use crate::sim::brain::Task;
        use crate::sim::uid::{EntityType, Uid};
        let bed = Point::new(6, 5);
        let mut world = night_with_beds(&[bed]);
        let mut human = owner(Point::new(5, 5), 60.0, bed);
        for _ in 0..600 {
            world.step(&mut human);
            if human.brain().is_asleep() {
                break;
            }
        }
        assert_eq!(human.center_position(), bed, "never got into bed: {:?}", human.brain_fields());
        let met = human.brain().attention().met();

        // A ring of people round the bed, so whichever way it faces, somebody
        // is in front of it.
        let mut n = 100;
        for dy in -2..=2 {
            for dx in -2..=2 {
                if i32::max(i32::abs(dx), i32::abs(dy)) == 2 {
                    world.occupancy.claim(bed.offset(dx, dy), Uid::new(EntityType::Human, n)).unwrap();
                    n += 1;
                }
            }
        }
        for _ in 0..3 * Priority::Med.period() {
            world.step(&mut human);
            assert!(human.brain().is_asleep(), "woke up");
        }
        assert_eq!(human.brain().perception().saw(), (0, 0));
        assert_eq!(human.brain().attention().met(), met);
    }

    fn step_until(world: &mut World, human: &mut Human, ticks: u32, done: impl Fn(&Human) -> bool) -> bool {
        for _ in 0..ticks {
            world.step(human);
            if done(human) {
                return true;
            }
        }
        false
    }

    /// Up for the toilet forty minutes into an hour: the forty minutes were
    /// sleep. The brain tells the task it is being dropped before the toilet
    /// takes over, and the task tells the body.
    #[test]
    fn a_sleeper_woken_for_the_toilet_keeps_the_rest_it_got() {
        let bed = Point::new(6, 5);
        let mut world = night_with_beds(&[bed]);
        prop_at(&mut world.map, "toilet", Point::new(10, 5));
        world.features = crate::sim::feature::Features::from_map(&world.map, &crate::map::utilities::Supply::everywhere(world.map.size()));
        let mut human = owner(Point::new(5, 5), 70.0, bed);
        human.biology_mut().unwrap().set_running(ProcessId::Bladder, true);
        assert!(step_until(&mut world, &mut human, 600, |h| h.brain().is_asleep()), "never got to sleep");
        let before = human.stats().stamina();

        // Forty world minutes asleep, then bursting.
        let forty_minutes = (40.0 * crate::sim::clock::MINUTE / TIME_SCALE / world.dt) as u32;
        for _ in 0..forty_minutes {
            world.step(&mut human);
        }
        assert!(human.brain().is_asleep(), "{:?}", human.brain_fields());
        human.set_bladder(95.0);
        assert!(step_until(&mut world, &mut human, 10, |h| h.brain().top_goal() == GoalId::Relieve));
        let gained = human.stats().stamina() - before;
        // Forty minutes is 2/3 of an hour's 12.5 net: about 8.3.
        assert!(gained > 6.0, "kept {gained} of forty minutes' sleep");
    }

    /// What `qa/a_day_in_the_office.json` caught: everything a body does
    /// running at once, and somewhere for each of it — a fridge, a toilet, a
    /// computer — and still a night in bed leaves somebody rested. With every
    /// need at a waking pace and an interrupted hour worth nothing, a sleeper
    /// was up for one of them every hour and stamina never got past 45.
    #[test]
    fn a_night_with_every_need_running_still_rests_a_tired_human() {
        let bed = Point::new(6, 8);
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "bed 1", bed);
        prop_at(&mut map, "fridge", Point::new(2, 3));
        prop_at(&mut map, "toilet", Point::new(11, 3));
        prop_at(&mut map, "computer", Point::new(7, 2));
        let mut world = World::new(map);
        world.clock = midnight();
        // A long day behind them: tired, peckish, thirsty, getting bored.
        let mut human = needy_human(Point::new(6, 6), 45.0, 45.0, 40.0);
        human.set_stamina(25.0);
        human.set_fun(55.0);
        assert!(human.set_home(bed, None));

        let night = (8.0 * HOUR / TIME_SCALE / world.dt) as u32;
        let mut asleep = 0;
        for _ in 0..night {
            world.step(&mut human);
            asleep += human.brain().is_asleep() as u32;
        }
        let stamina = human.stats().stamina();
        assert!(stamina >= 70.0, "stamina {stamina} after a night; asleep {asleep} of {night} ticks");
        assert!(asleep > night / 2, "asleep only {asleep} of {night} ticks");
    }

    #[test]
    fn a_tired_human_at_night_walks_to_its_own_bed_and_sleeps_in_it() {
        let bed = Point::new(11, 7);
        let mut world = night_with_beds(&[bed]);
        let mut human = owner(Point::new(2, 2), 60.0, bed);

        let mut went = false;
        for _ in 0..3000 {
            world.step(&mut human);
            went |= human.brain().top_goal() == GoalId::Sleep;
            if world.sleeps() > 0 {
                break;
            }
        }
        assert!(went, "tiredness at night should have put the bed in charge");
        assert!(
            world.log_contains("slept in the bed at 11, 7"),
            "never got there; the brain says {:?}",
            human.brain_fields()
        );
        assert_eq!(human.center_position(), bed, "should be lying in it, not beside it");
        assert!(human.stats().stamina() > 40.0, "stamina {}", human.stats().stamina());
    }

    #[test]
    fn a_human_goes_to_its_own_bed_even_when_another_is_nearer() {
        let (near, own) = (Point::new(4, 3), Point::new(10, 7));
        let mut world = night_with_beds(&[near, own]);
        let mut human = owner(Point::new(3, 3), 80.0, own);

        run_until_slept(&mut world, &mut human, 3000);
        assert!(world.log_contains("slept in the bed at 10, 7"), "lines {:?}", world.lines());
        assert!(!world.log_contains("slept in the bed at 4, 3"), "that one is not its own");
    }

    /// The whole reason for chunks of an hour: the routine decides how many.
    #[test]
    fn a_night_is_several_stretches_in_the_same_bed_until_the_morning() {
        let bed = Point::new(5, 5);
        let mut world = night_with_beds(&[bed]);
        let mut human = owner(Point::new(4, 5), 80.0, bed);

        // Five hours of world is a hundred and fifty watched seconds.
        for _ in 0..(5.0 * HOUR / TIME_SCALE * 64.0) as u32 {
            world.step(&mut human);
        }
        assert!(world.sleeps() >= 4, "{} stretches; brain {:?}", world.sleeps(), human.brain_fields());
        assert_eq!(human.center_position(), bed, "never left the bed");
        assert_eq!(human.brain().top_goal(), GoalId::Sleep, "and still asleep: it is night");
    }

    #[test]
    fn a_sleeper_rested_in_the_morning_gets_up_and_goes_back_to_wandering() {
        let bed = Point::new(5, 5);
        let mut world = night_with_beds(&[bed]);
        let mut human = owner(Point::new(4, 5), 40.0, bed);

        // Four and a half hours in bed is four stretches, and three are
        // what it takes to be rested from here.
        for _ in 0..(4.5 * HOUR / TIME_SCALE * 64.0) as u32 {
            world.step(&mut human);
        }
        assert!(human.stats().tiredness() <= RESTED + 1.0, "{}", human.stats().tiredness());
        assert_eq!(human.brain().top_goal(), GoalId::Sleep, "rested, but it is still night");

        world.clock = Clock::after_watching(0.0);
        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Wander, "morning: up and about");
    }

    #[test]
    fn a_tired_human_in_the_afternoon_does_not_go_to_bed() {
        let bed = Point::new(5, 5);
        let mut world = night_with_beds(&[bed]);
        world.clock = Clock::after_watching(0.0);
        let mut human = owner(Point::new(2, 2), SLEEPY + 20.0, bed);

        for _ in 0..500 {
            world.step(&mut human);
        }
        assert_eq!(world.sleeps(), 0);
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
    }

    #[test]
    fn an_exhausted_human_goes_to_bed_even_in_the_afternoon() {
        let bed = Point::new(5, 5);
        let mut world = night_with_beds(&[bed]);
        world.clock = Clock::after_watching(0.0);
        let mut human = owner(Point::new(2, 2), 80.0, bed);

        // A stretch is thirty watched seconds, 1920 ticks, and the walk there
        // is on top of that.
        run_until_slept(&mut world, &mut human, 4000);
        assert!(world.sleeps() > 0, "brain {:?}", human.brain_fields());
    }

    #[test]
    fn a_tired_human_with_no_bed_in_the_world_goes_back_to_wandering() {
        let mut world = night_with_beds(&[]);
        let mut human = tired_human(Point::new(6, 6), 95.0);
        let start = human.position();

        for _ in 0..200 {
            world.step(&mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Sleep) > 0, "the bed should be held off");
        assert_ne!(human.position(), start, "and it should be walking about meanwhile");
    }

    /// **The default.** A bed is somebody's, and somebody who merely wants to
    /// sleep does not take one that is not theirs.
    #[test]
    fn a_human_with_no_bed_of_its_own_does_not_sleep_in_a_free_one_short_of_critical() {
        let mut world = night_with_beds(&[Point::new(8, 5)]);
        let mut human = only_tired(Point::new(4, 5), CRITICALLY_TIRED - 5.0);

        for _ in 0..1500 {
            world.step(&mut human);
        }
        assert_eq!(world.sleeps(), 0, "a free bed, and it is nobody's");
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Sleep) > 0, "held off, not retried every tick");
    }

    /// **Critical is the exception.**
    #[test]
    fn a_critically_tired_human_with_no_bed_of_its_own_sleeps_in_a_free_one() {
        let bed = Point::new(10, 7);
        let mut world = night_with_beds(&[bed]);
        let mut human = only_tired(Point::new(3, 3), CRITICALLY_TIRED + 3.0);

        run_until_slept(&mut world, &mut human, 3000);
        assert!(world.log_contains("slept in the bed at 10, 7"), "lines {:?}", world.lines());
    }

    #[test]
    fn a_critically_tired_human_picks_among_the_free_beds_and_not_the_taken_ones() {
        let (taken, free) = (Point::new(4, 3), Point::new(10, 7));
        let mut world = night_with_beds(&[taken, free]);
        world
            .occupancy
            .claim(taken, Uid::new(EntityType::Human, 999))
            .expect("empty at the start");
        let mut human = only_tired(Point::new(3, 3), CRITICALLY_TIRED + 3.0);

        run_until_slept(&mut world, &mut human, 3000);
        assert!(world.log_contains("slept in the bed at 10, 7"), "lines {:?}", world.lines());
        assert!(!world.log_contains("slept in the bed at 4, 3"));
    }

    /// The beds are chosen at random — from a per-unit, per-tick stream, so
    /// the same unit on the same tick chooses the same, and different units do
    /// not all pile into the first.
    #[test]
    fn critically_tired_humans_spread_across_the_free_beds() {
        let beds = [Point::new(4, 3), Point::new(8, 3), Point::new(4, 7), Point::new(8, 7)];
        let mut chosen = std::collections::BTreeSet::new();
        for id in 0..24u64 {
            let mut world = night_with_beds(&beds);
            let mut rng = <rand::rngs::SmallRng as rand::SeedableRng>::seed_from_u64(id);
            let mut human = Human::new(Uid::new(EntityType::Human, 100 + id), Point::new(6, 5), &mut rng);
            human.set_hunger(0.0);
            human.set_thirst(0.0);
            human.set_bladder(0.0);
            human.set_fun(100.0);
            human.set_stamina(100.0 - (CRITICALLY_TIRED + 3.0));
            for _ in 0..3000 {
                world.step(&mut human);
                if world.sleeps() > 0 {
                    break;
                }
            }
            let line = world.lines().iter().find(|line| line.contains("slept in the bed")).cloned();
            chosen.insert(line.unwrap_or_default().split("bed at ").nth(1).unwrap_or("none").to_string());
        }
        assert!(chosen.len() > 1, "twenty-four different humans all chose {chosen:?}");
        assert!(!chosen.contains("none"), "somebody never slept: {chosen:?}");
    }

    /// Once in a bed a critical sleeper stays in it as its tiredness falls
    /// below critical, rather than getting up to go and find its own.
    #[test]
    fn a_critical_sleeper_stays_in_the_bed_it_took_as_its_tiredness_falls() {
        let beds = [Point::new(5, 5), Point::new(8, 5)];
        let mut world = night_with_beds(&beds);
        let mut human = only_tired(Point::new(6, 5), CRITICALLY_TIRED + 5.0);

        for _ in 0..(5.0 * HOUR / TIME_SCALE * 64.0) as u32 {
            world.step(&mut human);
        }
        let beds_slept_in: std::collections::BTreeSet<&str> = world
            .lines()
            .iter()
            .filter_map(|line| line.split("slept in the bed at ").nth(1))
            .collect();
        assert!(world.sleeps() >= 3, "{} stretches; brain {:?}", world.sleeps(), human.brain_fields());
        assert_eq!(beds_slept_in.len(), 1, "moved beds mid-night: {beds_slept_in:?}");
    }

    #[test]
    fn an_owner_whose_bed_is_taken_does_not_take_another_until_it_is_critical() {
        let (own, other) = (Point::new(4, 3), Point::new(10, 7));
        let mut world = night_with_beds(&[own, other]);
        world
            .occupancy
            .claim(own, Uid::new(EntityType::Human, 999))
            .expect("empty at the start");
        let mut human = owner(Point::new(3, 3), 60.0, own);

        for _ in 0..1500 {
            world.step(&mut human);
        }
        assert_eq!(world.sleeps(), 0, "its own is taken, and the other is nobody's to take");
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
    }

    #[test]
    fn an_owner_whose_bed_is_taken_and_is_critical_takes_a_free_one() {
        let (own, other) = (Point::new(4, 3), Point::new(10, 7));
        let mut world = night_with_beds(&[own, other]);
        world
            .occupancy
            .claim(own, Uid::new(EntityType::Human, 999))
            .expect("empty at the start");
        let mut human = owner(Point::new(3, 3), CRITICALLY_TIRED + 3.0, own);

        run_until_slept(&mut world, &mut human, 3000);
        assert!(world.log_contains("slept in the bed at 10, 7"), "lines {:?}", world.lines());
    }

    #[test]
    fn a_bed_that_stays_taken_is_given_up_on_rather_than_retried_forever() {
        let bed = Point::new(6, 6);
        let mut world = night_with_beds(&[bed]);
        world
            .occupancy
            .claim(bed, Uid::new(EntityType::Human, 999))
            .expect("empty at the start");
        let mut human = owner(Point::new(5, 6), 95.0, bed);

        for _ in 0..1000 {
            world.step(&mut human);
        }
        assert_eq!(world.sleeps(), 0);
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
        assert!(human.brain().goals().cooldown(GoalId::Sleep) > 0, "the bed should be held off");
    }

    #[test]
    fn a_bed_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for i in 7..12 {
            map.set_terrain(Point::new(i, 7), WALL);
            map.set_terrain(Point::new(7, i), WALL);
        }
        let bed = Point::new(10, 10);
        prop_at(&mut map, "bed 1", bed);
        let mut world = World::new(map);
        world.clock = midnight();
        let mut human = owner(Point::new(2, 2), 90.0, bed);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..1000 {
            world.step(&mut human);
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert_eq!(world.sleeps(), 0);
        assert!(routes < 100, "{routes} routes in 1000 ticks");
    }

    #[test]
    fn energy_switched_off_means_no_bed_however_tired() {
        let bed = Point::new(5, 5);
        let mut world = night_with_beds(&[bed]);
        let mut human = owner(Point::new(4, 5), 95.0, bed);
        human.biology_mut().unwrap().set_running(ProcessId::Energy, false);

        for _ in 0..500 {
            world.step(&mut human);
        }
        assert_eq!(world.sleeps(), 0);
        assert_eq!(human.brain().top_goal(), GoalId::Wander);
    }

    /// The ordering the night rests on: a sleeper wakes for the toilet, and
    /// goes back to bed afterwards, and neither is lost.
    #[test]
    fn a_sleeper_with_a_bursting_bladder_gets_up_for_the_toilet_and_goes_back_to_bed() {
        let bed = Point::new(3, 3);
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "bed 1", bed);
        prop_at(&mut map, "toilet", Point::new(10, 7));
        let mut world = World::new(map);
        world.clock = midnight();
        let mut human = tired_human(Point::new(2, 3), 60.0);
        assert!(human.set_home(bed, None));
        human.set_bladder(95.0);

        for _ in 0..8000 {
            world.step(&mut human);
            if world.reliefs() > 0 && world.sleeps() > 0 {
                break;
            }
        }
        assert!(world.reliefs() > 0, "never used the toilet; brain {:?}", human.brain_fields());
        assert!(world.sleeps() > 0, "never got to bed; brain {:?}", human.brain_fields());
        // The bladder was the worse of the two, so it came first.
        let line = |needle| world.lines().iter().position(|line| line.contains(needle));
        assert!(line("used the toilet") < line("slept in the bed"), "lines {:?}", world.lines());
    }
}
