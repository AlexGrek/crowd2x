//! [`Work`]: sit at one's own desk and work it for a while — or stand at
//! one's own guard post and keep watch, or by the power plant's control
//! panel, which is the same task at another workplace ([`Work::place`],
//! [`STRETCHES`]).
//!
//! [`UseComputer`](super::UseComputer)'s shape — a prop used from beside it,
//! time passing and nothing changing hands — with what the body gets out of it
//! the other way round: not [`Event::Entertained`] but [`Event::Worked`],
//! which tires and is no fun.
//!
//! One task is **one stretch at the desk**, an hour of world at the length
//! `WorkGoal` queues it, and a shift is several of them, the way a night is
//! several stretches of sleep. A stretch cut short — the shift ended, a
//! bursting bladder took over — still counts for what was worked of it
//! ([`TaskExecutor::interrupted`]): an hour's tiredness is the hour, not the
//! finishing of it.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::TIME_SCALE;
use crate::sim::feature::FeatureKind;

use super::super::action::{Action, ActionState};
use super::super::task::{self, SavedTask, Task, TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Work {
    /// The desk's cell — the computer, used from **beside** it like any
    /// [`Access::Beside`](crate::sim::feature::Access::Beside) feature, so
    /// this is what the worker is turned towards, never where it walks to.
    pub desk: Point,
    /// How long this stretch lasts, in **watched** seconds. The body is told
    /// it in world seconds ([`TIME_SCALE`] more).
    pub seconds: f32,
    /// What the workplace is, and has to stay for the stretch to go on: a
    /// computer (`Workstation`, which needs power), a guard post, a control
    /// panel — one of [`STRETCHES`]. The same work at each, as tiring and as
    /// little fun.
    pub place: FeatureKind,
}

/// Every workplace a job is worked at by standing at it — its job's goal is
/// `WorkGoal` — and how a stretch there reads: what it is, which is also the
/// name it is saved under (`"keep watch"`), and what it was, for the log
/// (`"kept watch"`). A new job of the kind needs its workplace here; a test
/// holds every such job to it.
pub const STRETCHES: [(FeatureKind, &str, &str); 3] = [
    (FeatureKind::Workstation, "work", "worked"),
    (FeatureKind::GuardPost, "keep watch", "kept watch"),
    (FeatureKind::ControlPanel, "stand by", "stood by"),
];

impl Work {
    /// What a stretch is ("keep watch") and was ("kept watch") at this
    /// workplace ([`STRETCHES`]).
    pub fn words(&self) -> (&'static str, &'static str) {
        STRETCHES
            .iter()
            .find(|(place, _, _)| *place == self.place)
            .map_or(("work", "worked"), |&(_, doing, done)| (doing, done))
    }
}

impl TaskExecutor for Work {
    /// **Checked every tick**, as [`UseComputer`](super::UseComputer) checks:
    /// within one step of the desk, a body to tire, and a desk that still
    /// works — a box switched off mid-shift turns the screen off and the
    /// stretch is over.
    ///
    /// The body hears [`Event::Worked`] once the action is over, or for as
    /// much as was done of it if it is cut short.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.desk) > 1 || ctx.biology.is_none() {
            return TaskResult::Failed;
        }
        if !ctx.think.features.has(self.place, self.desk) {
            return TaskResult::Failed;
        }

        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.desk, self.seconds);
            return TaskResult::Executing;
        }

        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked {
                        world_seconds: self.seconds * TIME_SCALE,
                    });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before the stretch was up: what was worked of it still
    /// tired the worker. Nothing while it had not sat down yet.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked {
                world_seconds: elapsed * TIME_SCALE,
            });
        }
    }

    fn describe(&self) -> String {
        format!("{} at the {} at {}, {}", self.words().0, self.place.prop(), self.desk.x, self.desk.y)
    }
}

impl Task {
    /// Work the desk in `desk`, from a cell beside it, for `seconds`.
    pub const fn work(desk: Point, seconds: f32) -> Task {
        Task::work_at(FeatureKind::Workstation, desk, seconds)
    }

    /// Keep watch at the guard post in `post`, from a cell beside it, for
    /// `seconds`: a guard's stretch, which is work.
    pub const fn keep_watch(post: Point, seconds: f32) -> Task {
        Task::work_at(FeatureKind::GuardPost, post, seconds)
    }

    /// A stretch at the workplace in `desk`, which is a `place` — one of
    /// [`STRETCHES`] — from a cell beside it, for `seconds`.
    pub const fn work_at(place: FeatureKind, desk: Point, seconds: f32) -> Task {
        Task::Work(Work { desk, seconds, place })
    }
}

/// Saved by what it is at its workplace ([`STRETCHES`]: `work`, `keep watch`,
/// `stand by`): the workplace and the seconds.
impl SavedTask for Work {
    fn save(&self) -> pb::TaskKind {
        task::named(self.words().0, Some(self.desk), self.seconds)
    }

    fn restore(saved: &pb::TaskKind) -> Option<Result<Work, Invalid>> {
        let ((cell, seconds), place) = match saved {
            pb::TaskKind::Work(task) => ((task.cell, task.seconds), FeatureKind::Workstation),
            pb::TaskKind::KeepWatch(task) => ((task.cell, task.seconds), FeatureKind::GuardPost),
            _ => STRETCHES
                .iter()
                .find_map(|&(place, doing, _)| Some((task::read_named(saved, doing)?, place)))?,
        };
        Some(task::cell_and_seconds(cell, seconds, "work").map(|(desk, seconds)| Work { desk, seconds, place }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::clock::{watched, HOUR};

    const DESK: Point = Point::new(6, 6);

    /// A unit in `cell` of a room with a working computer at [`DESK`], every
    /// process's level held where it is: the rig's biology moves only with
    /// events, so what a stretch costs is exact.
    fn rig_at(cell: Point) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "computer", DESK);
        let mut rig = Rig::new(map, cell);
        rig.biology.edit(|stats| stats.with_stamina(80.0).with_fun(40.0));
        rig
    }

    #[test]
    fn working_a_desk_from_across_the_room_fails_without_starting() {
        let mut rig = rig_at(Point::new(1, 1));
        let mut task = Task::work(DESK, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.biology.stats().stamina(), 80.0);
    }

    #[test]
    fn a_stretch_at_the_desk_tires_only_once_it_is_over_and_is_no_fun() {
        let mut rig = rig_at(Point::new(5, 6));
        let mut task = Task::work(DESK, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.biology.stats().stamina(), 80.0, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert!(rig.biology.stats().stamina() < 80.0, "{}", rig.biology.stats().stamina());
        assert_eq!(rig.biology.stats().fun(), 40.0, "work is no fun");
    }

    /// Called away half way through an hour: half an hour's work, not none.
    #[test]
    fn a_stretch_cut_short_still_tired_the_worker_for_what_was_done_of_it() {
        let mut rig = rig_at(Point::new(5, 6));
        let mut task = Task::work(DESK, watched(HOUR));
        let half = (watched(HOUR) / 2.0 / rig.world.dt) as usize;
        for _ in 0..half {
            assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        }
        assert_eq!(rig.biology.stats().stamina(), 80.0, "nothing until it is over or cut short");
        task.interrupted(&rig.action, Some(&mut rig.biology));
        let spent = 80.0 - rig.biology.stats().stamina();
        let hour = crate::sim::biology::energy::WORK_FATIGUE_PER_SECOND * HOUR;
        assert!((spent - hour / 2.0).abs() < 0.05, "spent {spent}, half an hour is {}", hour / 2.0);
    }

    #[test]
    fn walking_away_mid_stretch_fails_it_and_costs_nothing() {
        let mut rig = rig_at(Point::new(5, 6));
        let mut task = Task::work(DESK, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().stamina(), 80.0);
    }

    #[test]
    fn losing_power_mid_stretch_ends_it() {
        let mut rig = rig_at(Point::new(5, 6));
        let mut task = Task::work(DESK, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let dead = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.features = crate::sim::feature::Features::from_map(&rig.world.map, &dead);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
    }

    /// The desk is used from beside it: the worker stays in the chair, and
    /// the computer is what it is interacting with — which is what lights the
    /// screen (`game::props`).
    #[test]
    fn working_never_moves_anybody_onto_the_desk_and_lights_its_screen() {
        let beside = Point::new(5, 6);
        let mut rig = rig_at(beside);
        let mut task = Task::work(DESK, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.action.interacting_with(), Some(DESK));
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.walk.body().center_position(), beside);
    }

    /// Every job worked by standing at its workplace — the work goal's — has
    /// a stretch there of its own: one it is described and logged by, and a
    /// name no other workplace's stretch is saved under.
    #[test]
    fn every_job_stood_at_has_a_stretch_of_its_own() {
        use crate::sim::brain::GoalId;
        use crate::sim::job::JobKind;
        let stood_at = JobKind::ALL.into_iter().filter(|kind| kind.goal() == GoalId::Work);
        for kind in stood_at {
            assert!(
                STRETCHES.iter().any(|&(place, _, _)| place == kind.workplace()),
                "a {} has no stretch at its {}",
                kind.name(),
                kind.workplace_name()
            );
        }
        for (i, (place, doing, _)) in STRETCHES.iter().enumerate() {
            assert!(STRETCHES[..i].iter().all(|(other, said, _)| other != place && said != doing), "{doing} twice");
        }
        let panel = Task::work_at(FeatureKind::ControlPanel, DESK, 1.0);
        assert_eq!(panel.describe(), "stand by at the control panel at 6, 6");
        assert_eq!(Task::keep_watch(DESK, 1.0).describe(), "keep watch at the guard post at 6, 6");
        assert_eq!(Task::work(DESK, 1.0).describe(), "work at the computer at 6, 6");
    }

    /// A control panel needs no power, as a guard post does not: a stretch
    /// at one goes on in a room with no wiring at all.
    #[test]
    fn standing_by_at_a_control_panel_needs_no_power() {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "control panel", DESK);
        let mut rig = Rig::new(map, Point::new(5, 6));
        let dead = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.features = crate::sim::feature::Features::from_map(&rig.world.map, &dead);
        let mut task = Task::work_at(FeatureKind::ControlPanel, DESK, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
    }
}
