//! [`WorkGoal`]: walk to one's own desk and work a shift at it — or to
//! one's own guard post and keep a watch there, or to the power plant's
//! control panel and stand by, which is the same goal: the job says which
//! ([`JobKind::workplace`](crate::sim::job::JobKind::workplace)), and the
//! stretch is a [`Work`] at it.
//!
//! [`PlayGoal`](super::PlayGoal)'s plan — [`MoveTo`](crate::sim::brain::tasks::MoveTo)
//! beside a computer, then a timed task at it — with two differences.
//!
//! **Which computer.** Not the nearest: the worker's own desk, handed out with
//! the job at spawn ([`Job::workplace`](crate::sim::job::Job)). There is no
//! other to go to, so a desk that cannot be got to is not swapped for the next
//! one along — the visit is given up on and the goal held off, and the worker
//! tries again when the hold runs out.
//!
//! **What an ending means.** One [`Work`](crate::sim::brain::tasks::Work) task
//! is an hour at the desk, and a shift is several of them: this goal reports
//! `Achieved` after each, and if the routine still wants work the next hour
//! starts from the chair. **How many hours happen is the routine's to say**
//! (`WorkRoutine`: the shift's hours, on its days) — this goal never reads the
//! clock. Between two stretches, or in the middle of one, a need urgent enough
//! to outrank work takes the worker away and work brings it back.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, HOUR};

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::Work;
use super::{stand_beside, Seek, Stand};

/// One stretch at the desk: an hour of world, thirty seconds of watching it —
/// as long as a stretch of sleep, and for the same reason: short enough that
/// something more pressing gets a look in between two.
pub const WORK_SECONDS: f32 = watched(HOUR);

/// Work a shift at one's own desk.
///
/// What it remembers is the desk it is going to and how the way there has
/// gone. Which desk is its own is not here — that is the job's, in the brain,
/// and outlives the goal being put down.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WorkGoal {
    seek: Seek,
}

impl WorkGoal {
    pub fn new() -> WorkGoal {
        WorkGoal::default()
    }

    /// The desk to work at: the job's, if it is somewhere to work right now.
    /// `None` with no job, or a desk whose computer has no power — which
    /// the goal treats as the expensive miss it is, rather than starting a
    /// stretch that fails the tick it starts, every tick. A guard's desk is
    /// its post.
    fn desk(ctx: &GoalCtx<'_>) -> Option<Point> {
        let job = ctx.job?;
        ctx.think.features.has(job.kind.workplace(), job.workplace).then_some(job.workplace)
    }

    /// Queue the rest **from what is true now**, on the back of the queue:
    /// walk beside the desk (unless already there), work it. `false` when
    /// there is nowhere to stand to work it.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(desk) = self.seek.target else {
            return false;
        };
        match stand_beside(ctx, desk) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let Some(job) = ctx.job else {
            return false;
        };
        let _ = ctx.tasks.push_back(Task::work_at(job.kind.workplace(), desk, WORK_SECONDS));
        true
    }

    fn forget(&mut self) {
        self.seek.forget();
    }

    /// One stretch done: say so, and start the next from scratch.
    fn worked(&mut self, ctx: &GoalCtx<'_>, task: Work) {
        let (uid, at) = (ctx.body.uid(), task.desk);
        let done = task.words().1;
        ctx.think.log.push(format!("{uid} {done} an hour at the {} at {}, {}", task.place.prop(), at.x, at.y));
        self.forget();
    }
}

impl GoalExecutor for WorkGoal {
    fn goal(&self) -> GoalId {
        GoalId::Work
    }

    fn save(&self) -> pb::Executor {
        let WorkGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// The half-walked route is gone, the desk is not — unless it is no
    /// longer this worker's: let go and hired again elsewhere, the desk it was
    /// on its way to is somebody else's business now.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.seek.target.is_none() {
            return;
        }
        if self.seek.target != ctx.job.map(|job| job.workplace) {
            self.forget();
            return;
        }
        self.seek.steady();
        if !self.plan(ctx) {
            self.forget();
        }
    }

    /// A stretch that ended the very tick something else took over — the
    /// shift ending on the hour — is heard here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if ctx.tasks.result() == TaskResult::Success
            && let Some(Task::Work(task)) = ctx.finished
        {
            self.worked(ctx, task);
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() || ctx.job.is_none() {
            // Nothing to tire, or nothing to do.
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: there
                // is no other desk that is this one's, so give up on the
                // visit and let the hold space out the next try.
                if self.seek.setback(ctx, |_| None) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Pulled away from the desk, or its power went. Cheap to work
                // out again next tick — and `desk` says so if it was power.
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, Some(Task::Work(task))) => {
                self.worked(ctx, task);
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: start, or start the next hour.
        let Some(desk) = WorkGoal::desk(ctx) else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(desk);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "desk",
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
    use crate::sim::clock::{Clock, DAY, TIME_SCALE};
    use crate::sim::job::Job;
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::uid::{EntityType, Uid};
    use crate::sim::{GameEntity, Human};

    /// `hours` o'clock on day `day` — day 1 is a Monday.
    fn at(day: u32, hours: f32) -> Clock {
        let world = (day - 1) as f64 * DAY as f64 + (hours as f64 - 8.0) * HOUR as f64;
        Clock::after_watching(world / TIME_SCALE as f64)
    }

    /// Ticks in `hours` of world at the test world's rate.
    fn ticks(world: &World, hours: f32) -> u32 {
        (hours * HOUR / TIME_SCALE / world.dt) as u32
    }

    /// An office worker at `desk` with nothing else on its mind: every need
    /// satisfied and every process but tiring switched off, so a test about a
    /// shift is not a race with five needs.
    fn worker(cell: Point, desk: Point) -> Human {
        let mut human = needy_human(cell, 0.0, 0.0, 0.0);
        human.set_stamina(100.0);
        let biology = human.biology_mut().unwrap();
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
            biology.set_running(process, false);
        }
        assert!(human.hire(Job::office(desk)));
        human
    }

    /// An office with one computer at `desk`, at `clock`.
    fn office(desk: Point, clock: Clock) -> World {
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "computer", desk);
        let mut world = World::new(map);
        world.clock = clock;
        world
    }

    /// Step, moving the world's clock on with the ticks as a `GameState`
    /// would: these tests are about the time of day.
    fn step(world: &mut World, human: &mut Human) {
        world.step_in_time(human);
    }

    #[test]
    fn an_office_worker_sets_off_at_half_past_eight_and_works_at_its_desk() {
        let desk = Point::new(11, 7);
        let mut world = office(desk, at(1, 8.4));
        let mut human = worker(Point::new(2, 2), desk);

        for _ in 0..ticks(&world, 0.05) {
            step(&mut world, &mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Work, "twenty past eight: not yet");

        world.clock = at(1, 8.55);
        let mut seated = false;
        for _ in 0..ticks(&world, 1.5) {
            step(&mut world, &mut human);
            seated |= human.brain().top_goal() == GoalId::Work
                && matches!(human.brain().current_task(), Some(Task::Work(_)));
            if world.works() > 0 {
                break;
            }
        }
        assert!(seated, "never sat down; brain {:?}", human.brain_fields());
        assert!(world.log_contains("worked an hour at the computer at 11, 7"), "{:?}", world.lines());
        assert_eq!(human.center_position().manhattan_distance(desk), 1, "beside the desk, not on it");
        assert_eq!(human.brain().top_goal(), GoalId::Work, "and still at it: it is the middle of the morning");
    }

    /// A guard on the night watch sets off at ten, two hours before it (a
    /// guard's commute), stands at
    /// its post rather than on it, keeps watch an hour at a time (a post
    /// needs no power), and says so as a watch, not a desk.
    #[test]
    fn a_guard_on_the_night_watch_sets_off_before_midnight_and_keeps_watch_at_its_post() {
        use crate::sim::job::WATCHES;
        let post = Point::new(11, 7);
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "guard post", post);
        let mut world = World::new(map);
        world.clock = at(1, 21.8);
        let mut human = worker(Point::new(2, 2), post);
        human.dismiss();
        assert!(human.hire(Job::guard(post, WATCHES[2])));

        for _ in 0..ticks(&world, 0.1) {
            step(&mut world, &mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Work, "a quarter to ten: not yet");

        world.clock = at(1, 22.05);
        let mut watching = false;
        for _ in 0..ticks(&world, 1.6) {
            step(&mut world, &mut human);
            watching |= matches!(human.brain().current_task(), Some(Task::Work(Work { place: crate::sim::feature::FeatureKind::GuardPost, .. })));
            if world.log_contains("kept watch an hour") {
                break;
            }
        }
        assert!(watching, "never stood at the post; brain {:?}", human.brain_fields());
        assert!(world.log_contains("kept watch an hour at the guard post at 11, 7"), "{:?}", world.lines());
        assert_eq!(world.works(), 0, "not an hour at a computer");
        assert!(world.clock.hours() < 8.0 || world.clock.hours() > 22.0, "the night");
        assert_eq!(human.center_position().manhattan_distance(post), 1, "beside the post, not on it");
        assert_eq!(human.brain().top_goal(), GoalId::Work, "and still at it");
    }

    /// A power plant worker on the day watch sets off at half past seven,
    /// stands by its control panel (which needs no power) an hour at a time,
    /// and says so as standing by — not as an hour at a computer or a watch.
    #[test]
    fn a_power_plant_worker_on_the_day_watch_stands_by_at_its_control_panel() {
        use crate::sim::feature::FeatureKind;
        use crate::sim::job::{JobKind, WATCHES};
        let panel = Point::new(11, 7);
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "control panel", panel);
        let mut world = World::new(map);
        world.clock = at(1, 7.3);
        let mut human = worker(Point::new(2, 2), panel);
        human.dismiss();
        assert!(human.hire(Job::new(JobKind::PowerPlantWorker, panel, WATCHES[0])));

        for _ in 0..ticks(&world, 0.1) {
            step(&mut world, &mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Work, "twenty past seven: not yet");

        world.clock = at(1, 7.55);
        let mut standing_by = false;
        for _ in 0..ticks(&world, 1.6) {
            step(&mut world, &mut human);
            standing_by |=
                matches!(human.brain().current_task(), Some(Task::Work(Work { place: FeatureKind::ControlPanel, .. })));
            if world.log_contains("stood by an hour") {
                break;
            }
        }
        assert!(standing_by, "never stood by the panel; brain {:?}", human.brain_fields());
        assert!(world.log_contains("stood by an hour at the control panel at 11, 7"), "{:?}", world.lines());
        assert_eq!(world.works(), 0, "not an hour at a computer");
        assert!(!world.log_contains("kept watch"), "nor a watch at a post");
        assert_eq!(human.center_position().manhattan_distance(panel), 1, "beside the panel, not on it");
        assert_eq!(human.brain().top_goal(), GoalId::Work, "and still at it");
    }

    /// The king sets off for court at half past eight, slowly, and holds it
    /// at its throne from ten: an hour's stretch beside the throne, logged as
    /// court held.
    #[test]
    fn the_king_holds_court_beside_its_throne() {
        use crate::sim::feature::FeatureKind;
        use crate::sim::job::{JobKind, COURT_HOURS};
        let throne = Point::new(11, 7);
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "throne", throne);
        let mut world = World::new(map);
        world.clock = at(1, 8.3);
        let mut human = worker(Point::new(2, 2), throne);
        human.dismiss();
        assert!(human.hire(Job::new(JobKind::King, throne, COURT_HOURS)));

        for _ in 0..ticks(&world, 0.1) {
            step(&mut world, &mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Work, "twenty past eight: not yet");

        world.clock = at(1, 9.9);
        let mut holding_court = false;
        for _ in 0..ticks(&world, 1.6) {
            step(&mut world, &mut human);
            holding_court |= matches!(human.brain().current_task(), Some(Task::Work(Work { place: FeatureKind::Throne, .. })));
            if world.log_contains("held court an hour") {
                break;
            }
        }
        assert!(holding_court, "never held court; brain {:?}", human.brain_fields());
        assert!(world.log_contains("held court an hour at the throne at 11, 7"), "{:?}", world.lines());
        assert_eq!(world.works(), 0, "not an hour at a computer");
        assert_eq!(human.center_position().manhattan_distance(throne), 1, "beside the throne, not on it");
    }

    #[test]
    fn at_five_o_clock_the_worker_stops_and_the_time_worked_still_tired_it() {
        let desk = Point::new(6, 5);
        let mut world = office(desk, at(2, 16.7));
        let mut human = worker(Point::new(5, 5), desk);
        // Sitting down.
        for _ in 0..ticks(&world, 0.2) {
            step(&mut world, &mut human);
        }
        assert!(matches!(human.brain().current_task(), Some(Task::Work(_))), "{:?}", human.brain_fields());
        let before = human.stats().stamina();

        for _ in 0..ticks(&world, 0.2) {
            step(&mut world, &mut human);
        }
        assert!(world.clock.hours() > 17.0);
        assert_eq!(human.brain().top_goal(), GoalId::Wander, "home time");
        let after = human.stats().stamina();
        let awake = (0.2 * HOUR) * crate::sim::biology::energy::ENERGY_PER_SECOND;
        assert!(before - after > awake, "a part-hour at the desk still tired it: {before} -> {after}");
    }

    #[test]
    fn nobody_goes_to_work_at_the_weekend() {
        let desk = Point::new(6, 5);
        for day in [6, 7] {
            let mut world = office(desk, at(day, 10.0));
            let mut human = worker(Point::new(2, 2), desk);
            for _ in 0..600 {
                step(&mut world, &mut human);
                assert_ne!(human.brain().top_goal(), GoalId::Work, "day {day}");
            }
            assert_eq!(world.works(), 0);
        }
    }

    #[test]
    fn somebody_with_no_job_never_goes_to_work() {
        let desk = Point::new(6, 5);
        let mut world = office(desk, at(1, 10.0));
        let mut human = needy_human(Point::new(2, 2), 0.0, 0.0, 0.0);
        for _ in 0..600 {
            step(&mut world, &mut human);
            assert_ne!(human.brain().top_goal(), GoalId::Work);
        }
    }

    #[test]
    fn a_desk_with_no_power_is_held_off_rather_than_retried_every_tick() {
        let desk = Point::new(6, 5);
        let mut world = office(desk, at(1, 10.0));
        world.features = crate::sim::feature::Features::from_map(
            &world.map,
            &crate::map::utilities::Supply::from_map(&world.map),
        );
        let mut human = worker(Point::new(2, 2), desk);
        for _ in 0..200 {
            step(&mut world, &mut human);
        }
        assert_eq!(world.works(), 0);
        assert_ne!(human.brain().top_goal(), GoalId::Work);
        assert!(human.brain().goals().cooldown(GoalId::Work) > 0, "the dead desk should be held off");
    }

    #[test]
    fn a_desk_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for i in 7..12 {
            map.set_terrain(Point::new(i, 7), WALL);
            map.set_terrain(Point::new(7, i), WALL);
        }
        let desk = Point::new(10, 10);
        prop_at(&mut map, "computer", desk);
        let mut world = World::new(map);
        world.clock = at(1, 10.0);
        let mut human = worker(Point::new(2, 2), desk);

        crate::sim::walker::ROUTES_ASKED.with(|asked| asked.set(0));
        for _ in 0..1000 {
            step(&mut world, &mut human);
        }
        let routes = crate::sim::walker::ROUTES_ASKED.with(|asked| asked.get());
        assert_eq!(world.works(), 0);
        assert!(routes < 100, "{routes} routes in 1000 ticks");
    }

    /// Let go on the way to one desk and hired at another: it goes to the new
    /// one, not to where it was heading.
    #[test]
    fn a_worker_hired_at_another_desk_goes_to_the_new_one() {
        let (old, new) = (Point::new(11, 7), Point::new(2, 7));
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "computer", old);
        prop_at(&mut map, "computer", new);
        let mut world = World::new(map);
        world.clock = at(1, 10.0);
        let mut human = worker(Point::new(6, 2), old);
        for _ in 0..20 {
            step(&mut world, &mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Work, "on its way to the old desk");

        assert!(human.dismiss().is_some());
        for _ in 0..5 {
            step(&mut world, &mut human);
        }
        assert_ne!(human.brain().top_goal(), GoalId::Work);
        assert!(human.hire(Job::office(new)));
        for _ in 0..ticks(&world, 1.3) {
            step(&mut world, &mut human);
            if world.works() > 0 {
                break;
            }
        }
        assert!(world.log_contains("worked an hour at the computer at 2, 7"), "{:?}", world.lines());
        assert!(!world.log_contains("at the computer at 11, 7"));
    }

    /// Somebody else at the desk already, on one side: the worker sits down
    /// on another, as at any computer.
    #[test]
    fn somebody_already_beside_the_desk_does_not_keep_the_worker_from_it() {
        let desk = Point::new(6, 6);
        let mut world = office(desk, at(1, 10.0));
        let taken = Point::new(5, 6);
        world.occupancy.claim(taken, Uid::new(EntityType::Human, 999)).unwrap();
        let mut human = worker(Point::new(2, 6), desk);
        for _ in 0..ticks(&world, 1.3) {
            step(&mut world, &mut human);
            if world.works() > 0 {
                break;
            }
        }
        assert!(world.works() > 0, "never worked; brain {:?}", human.brain_fields());
        assert_ne!(human.center_position(), taken);
    }

    /// The ordering work is pitched at: a need only just committed to waits
    /// for the shift; one that is urgent takes the worker away from the desk,
    /// and the desk has it back afterwards.
    #[test]
    fn a_peckish_worker_keeps_working_and_a_bursting_one_goes_and_comes_back() {
        let desk = Point::new(6, 5);
        let mut map = Map::new(Size::new(14, 10), FLOOR);
        prop_at(&mut map, "computer", desk);
        prop_at(&mut map, "toilet", Point::new(11, 2));
        let mut world = World::new(map);
        world.clock = at(1, 10.0);
        let mut human = worker(Point::new(5, 5), desk);
        human.biology_mut().unwrap().set_running(ProcessId::Bladder, true);
        human.set_bladder(crate::sim::brain::routines::BURSTING);

        for _ in 0..200 {
            step(&mut world, &mut human);
        }
        assert_eq!(human.brain().top_goal(), GoalId::Work, "bursting is not urgent enough to leave the desk");

        human.set_bladder(85.0);
        let mut relieved = false;
        let mut back = false;
        for _ in 0..ticks(&world, 1.0) {
            step(&mut world, &mut human);
            relieved |= world.reliefs() > 0;
            if relieved && matches!(human.brain().current_task(), Some(Task::Work(_))) {
                back = true;
                break;
            }
        }
        assert!(relieved, "never went; brain {:?}", human.brain_fields());
        assert!(back, "never came back to the desk; brain {:?}", human.brain_fields());
    }
}
