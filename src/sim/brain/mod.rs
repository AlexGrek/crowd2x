//! A unit's mind: [`Brain`], and the fixed pipeline that runs it.
//!
//! # Layers
//!
//! ```text
//! perception   what it can see                  (a stub, for now)
//! memory       what it remembers                (empty, for now)
//! routines     arrange the priority list        KeepFed, StayBusy
//! goals        the list, and who is in charge   Idle, Wander, Eat
//! tasks        the plan the goal in charge made MoveTo, Interact, Wait
//! action       the task being carried out       pathfinding and timing only
//! ```
//!
//! Each layer only talks to the one below it. A routine does not queue tasks,
//! a goal does not ask for routes, an action does not decide anything — which
//! is what lets two goals take turns without either knowing about the other.
//!
//! # The pipeline
//!
//! It all runs in [`GameEntity::react`](super::GameEntity::react): the only
//! `&mut self` hook, and the one that runs after the whole crowd has moved.
//! `think` keeps what it had — walk the current route — so it stays arithmetic
//! and stays parallel.
//!
//! ```text
//! 0.  perception.observe(..)                          stub, costs nothing
//! 0b. advance the current action (this tick's MoveOutcome)
//!        -> Running | Finished | Failed, which becomes the TaskResult
//! 1.  every routine arranges the priority list        every tick
//! 3.  settle the top; if it changed:
//!        old.deprioritized() -> tasks.clear() -> walk.halt() -> new.prioritized()
//! 4.  if (goal changed || no action running): top executor.process(last result)
//! 6.  if no action is running, pop the front task and start its action
//! ```
//!
//! Advancing the action comes first because step 4's gate needs to know
//! whether it is still running; starting the next one comes last because
//! doing it before step 4 would cost an idle tick at every task boundary.
//!
//! # Planning cadence, which is what matters
//!
//! A route is asked for in one place — step 6, starting a `MoveTo` — so the
//! number of far searches is the number of walks started, and a walk takes
//! many ticks. An executor is only asked for more work when an action ends or
//! the goal in charge changes, and a goal that finds out expensively that it
//! cannot be done is held off ([`goal::BLOCKED_TICKS`], doubling). Per-agent,
//! per-tick planning is what killed the earlier prototype, and
//! `a_brain_never_asks_for_a_route_more_than_once_a_task` is the test that
//! says this is not that.

pub mod action;
pub mod goal;
pub mod goals;
pub mod memory;
pub mod perception;
pub mod routine;
pub mod routines;
pub mod task;

pub use action::{Action, ActionState};
pub use goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress, Goals};
pub use memory::{Memory, Recall};
pub use perception::Perception;
pub use routine::{Routine, RoutineCtx};
pub use task::{Task, TaskResult, Tasks};

use super::entity::Think;
use super::item::ItemKind;
use super::stats::Stats;
use super::uid::Uid;
use super::walker::Walker;
use super::MoveOutcome;

use goals::{EatGoal, WanderGoal};
use routines::{KeepFedRoutine, StayBusyRoutine};

pub struct Brain {
    perception: Perception,
    memory: Memory,
    /// Run in this order every tick. A fixed array rather than a `Vec`, and the
    /// order is fixed at construction: an order that depended on anything at
    /// runtime is a determinism bug waiting for a thousand-tick test to find it.
    routines: [Option<Box<dyn Routine>>; Brain::MAX_ROUTINES],
    goals: Goals,
    /// One executor per [`GoalId`] this brain can pursue, `None` for one it
    /// cannot. **Owned for the entity's whole life** — which is the whole
    /// answer to how an executor keeps state across being put down.
    executors: [Option<Box<dyn GoalExecutor>>; GoalId::COUNT],
    tasks: Tasks,
    action: Action,
    /// The task `action` is carrying out.
    task: Option<Task>,
    /// The task that ended since the executor was last asked, and what
    /// refused it if a body did — handed over in [`GoalCtx`] and then
    /// forgotten.
    finished: Option<Task>,
    blocked_by: Option<Uid>,
    /// How the last task to end went, kept for a debugger after the executor
    /// has been told.
    last_result: TaskResult,
}

impl Brain {
    pub const MAX_ROUTINES: usize = 4;

    /// A brain with these routines, run in the order given, and these
    /// executors, filed under the goal each pursues.
    ///
    /// Panics on more than [`Brain::MAX_ROUTINES`] routines or two executors
    /// for one goal: both are a kind built wrong, found the first time one is
    /// spawned.
    pub fn new(
        routines: impl IntoIterator<Item = Box<dyn Routine>>,
        executors: impl IntoIterator<Item = Box<dyn GoalExecutor>>,
    ) -> Brain {
        let mut brain = Brain {
            perception: Perception,
            memory: Memory::new(),
            routines: [const { None }; Brain::MAX_ROUTINES],
            goals: Goals::new(),
            executors: [const { None }; GoalId::COUNT],
            tasks: Tasks::new(),
            action: Action::None,
            task: None,
            finished: None,
            blocked_by: None,
            last_result: TaskResult::InProgress,
        };
        for (i, routine) in routines.into_iter().enumerate() {
            assert!(i < Brain::MAX_ROUTINES, "more than {} routines", Brain::MAX_ROUTINES);
            brain.routines[i] = Some(routine);
        }
        for executor in executors {
            let slot = &mut brain.executors[executor.goal() as usize];
            assert!(slot.is_none(), "two executors for {:?}", executor.goal());
            *slot = Some(executor);
        }
        brain
    }

    /// A person: keeps fed, stays busy; wanders, eats.
    pub fn human() -> Brain {
        Brain::new(
            [
                Box::new(KeepFedRoutine::new()) as Box<dyn Routine>,
                Box::new(StayBusyRoutine),
            ],
            [
                Box::new(WanderGoal::new()) as Box<dyn GoalExecutor>,
                Box::new(EatGoal::new()),
            ],
        )
    }

    /// A dog: stays busy; wanders. Same machinery, fewer parts.
    pub fn dog() -> Brain {
        Brain::new(
            [Box::new(StayBusyRoutine) as Box<dyn Routine>],
            [Box::new(WanderGoal::new()) as Box<dyn GoalExecutor>],
        )
    }

    /// **The pipeline.** See the module docs for the steps and their order.
    ///
    /// `walk` is the body being driven; `stats` and `carried` are `None` for a
    /// kind that has no needs or no hands.
    pub fn react(
        &mut self,
        ctx: &Think<'_>,
        outcome: MoveOutcome,
        walk: &mut Walker,
        stats: Option<&mut Stats>,
        carried: Option<&mut Option<ItemKind>>,
    ) {
        // Disjoint field borrows, so the compiler is what checks the steps
        // below do not overlap — the same trick `process_pass` uses.
        let Brain {
            perception,
            memory,
            routines,
            goals,
            executors,
            tasks,
            action,
            task,
            finished,
            blocked_by,
            last_result,
        } = self;
        let body = *walk.body();
        let here = body.center_position();

        // 0. Look around.
        perception.observe(ctx, &body);

        // 0b. Advance the action. A walk is the walker's to judge.
        let running = !action.is_none();
        let state = match *action {
            Action::Move { .. } => {
                action.advance(ctx.dt, here);
                walk.advance(ctx, outcome)
            }
            _ => action.advance(ctx.dt, here),
        };
        match state {
            ActionState::Running => tasks.set_result(TaskResult::Executing),
            ActionState::Finished | ActionState::Failed if running => {
                let result = if state == ActionState::Finished {
                    TaskResult::Success
                } else {
                    TaskResult::Failed
                };
                *blocked_by = match (result, *action) {
                    (TaskResult::Failed, Action::Move { .. }) => outcome.obstacle(),
                    _ => None,
                };
                tasks.set_result(result);
                *last_result = result;
                *finished = task.take();
                *action = Action::None;
                walk.halt();
            }
            ActionState::Finished | ActionState::Failed => {}
        }

        // 1. Arrange the list, from scratch.
        goals.clear_priorities();
        goals.tick_cooldowns();
        {
            let routine_ctx = RoutineCtx {
                think: ctx,
                body: &body,
                stats: stats.as_deref(),
            };
            for routine in routines.iter_mut().flatten() {
                routine.arrange(&routine_ctx, goals);
            }
        }

        let mut goal_ctx = GoalCtx {
            think: ctx,
            body: &body,
            perception,
            stats,
            memory,
            tasks,
            carried,
            blocked_by: *blocked_by,
            finished: *finished,
        };

        // 3. Settle, and hand over if the top changed.
        let changed = goals.settle();
        if let Some((old, new)) = changed {
            if let Some(executor) = executors[old as usize].as_deref_mut() {
                executor.deprioritized(&mut goal_ctx);
            }
            goal_ctx.tasks.clear();
            walk.halt();
            *action = Action::None;
            *task = None;
            // What ended belonged to the goal that queued it.
            goal_ctx.tasks.set_result(TaskResult::InProgress);
            goal_ctx.finished = None;
            goal_ctx.blocked_by = None;
            if let Some(executor) = executors[new as usize].as_deref_mut() {
                executor.prioritized(&mut goal_ctx);
            }
        }

        // 4. Ask the goal in charge for more, if there is room for more.
        let top = goals.top();
        if changed.is_some() || action.is_none() {
            if let Some(executor) = executors[top as usize].as_deref_mut() {
                let last = goal_ctx.tasks.result();
                match executor.process(&mut goal_ctx, last) {
                    GoalProgress::Working => {
                        if last == TaskResult::Success {
                            goals.progressed(top);
                        }
                    }
                    GoalProgress::Achieved => goals.achieved(top),
                    GoalProgress::Blocked => {
                        goals.block(top);
                        goal_ctx.tasks.clear();
                    }
                }
            }
            // Told, so forgotten: the next call hears about the next task.
            goal_ctx.tasks.set_result(TaskResult::InProgress);
            *finished = None;
            *blocked_by = None;
        }

        // 6. Start the next task.
        if action.is_none()
            && let Some(next) = goal_ctx.tasks.pop_front()
        {
            let routed = match next {
                Task::MoveTo { cell } => walk.route_to(ctx, cell),
                Task::Interact { .. } | Task::Wait { .. } => true,
            };
            if routed {
                *action = Action::start(next);
                *task = Some(next);
                goal_ctx.tasks.set_result(TaskResult::Executing);
            } else {
                // No way there. Reported next tick like any other ending, so
                // an executor hears about every task the same way.
                goal_ctx.tasks.set_result(TaskResult::Failed);
                *last_result = TaskResult::Failed;
                *finished = Some(next);
                *blocked_by = None;
            }
        }
    }

    pub fn goals(&self) -> &Goals {
        &self.goals
    }

    pub fn top_goal(&self) -> GoalId {
        self.goals.top()
    }

    pub fn action(&self) -> Action {
        self.action
    }

    pub fn current_task(&self) -> Option<Task> {
        self.task
    }

    pub fn tasks(&self) -> &Tasks {
        &self.tasks
    }

    pub fn last_result(&self) -> TaskResult {
        self.last_result
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// What a brain would tell a debugger, for the brains menu.
    ///
    /// Allocates freely: it is asked about the one unit somebody has selected,
    /// once a frame, and never in a tick.
    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let top = self.goals.top();
        let mut fields = vec![
            ("goal", top.name().to_string()),
            (
                "priority",
                self.goals
                    .ranked()
                    .iter()
                    .map(|(goal, priority)| format!("{} {priority:.2}", goal.name()))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ];
        let held: Vec<String> = GoalId::ALL
            .iter()
            .filter(|&&goal| self.goals.cooldown(goal) > 0)
            .map(|&goal| format!("{} {}t", goal.name(), self.goals.cooldown(goal)))
            .collect();
        if !held.is_empty() {
            fields.push(("held off", held.join(", ")));
        }
        fields.extend([
            (
                "task",
                self.task.map_or("none".to_string(), |task| task.describe()),
            ),
            ("queue", self.tasks.len().to_string()),
            (
                "result",
                if self.action.is_none() {
                    self.last_result.name().to_string()
                } else {
                    TaskResult::Executing.name().to_string()
                },
            ),
            ("action", self.action.describe()),
            (
                "routines",
                self.routines
                    .iter()
                    .flatten()
                    .map(|routine| routine.name())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ]);
        for routine in self.routines.iter().flatten() {
            fields.extend(routine.debug_fields());
        }
        if let Some(executor) = self.executors[top as usize].as_deref() {
            fields.extend(executor.debug_fields());
        }
        fields.push((
            "memory",
            if self.memory.is_empty() {
                "empty".to_string()
            } else {
                self.memory
                    .iter()
                    .map(|(key, recall)| format!("{key} {}", recall.describe()))
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        ));
        fields
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::map::{Map, Point, Size, FLOOR};
    use crate::sim::testing::World;
    use crate::sim::uid::EntityType;
    use crate::sim::walker::ROUTES_ASKED;
    use crate::sim::{GameEntity, Intent};
    use rand::RngExt;

    /// A thing with a walker and a brain and nothing else, so the brain can be
    /// driven through `World::step` with test-double routines and executors.
    struct Puppet {
        walk: Walker,
        brain: Brain,
    }

    impl Puppet {
        fn new(cell: Point, brain: Brain) -> Puppet {
            Puppet {
                walk: Walker::new(Uid::new(EntityType::Human, 5), cell, 4.0),
                brain,
            }
        }
    }

    impl GameEntity for Puppet {
        fn body(&self) -> &crate::sim::Body {
            self.walk.body()
        }
        fn body_mut(&mut self) -> &mut crate::sim::Body {
            self.walk.body_mut()
        }
        fn think(&self, ctx: &Think<'_>) -> Intent {
            self.walk.think(ctx)
        }
        fn apply(&mut self, intent: &Intent) {
            self.walk.apply(intent);
        }
        fn react(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) {
            self.brain.react(ctx, outcome, &mut self.walk, None, None);
        }
    }

    /// Raises one goal to a priority read from a shared dial, so a test can
    /// turn a goal up and down from outside the brain.
    struct Dial {
        goal: GoalId,
        level: Arc<Mutex<f32>>,
    }

    impl Routine for Dial {
        fn name(&self) -> &'static str {
            "dial"
        }
        fn arrange(&mut self, _ctx: &RoutineCtx<'_>, goals: &mut Goals) {
            goals.raise_to(self.goal, *self.level.lock().unwrap());
        }
    }

    /// Records every call made to it, in order, into a shared journal.
    struct Recorder {
        goal: GoalId,
        journal: Arc<Mutex<Vec<String>>>,
        /// Stage, kept in a field — the thing under test is that it survives.
        stage: u32,
        /// What to queue each time it is processed with nothing queued.
        plan: Option<Task>,
    }

    impl Recorder {
        fn note(&self, what: &str) {
            self.journal
                .lock()
                .unwrap()
                .push(format!("{} {what}", self.goal.name()));
        }
    }

    impl GoalExecutor for Recorder {
        fn goal(&self) -> GoalId {
            self.goal
        }
        fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
            assert!(ctx.tasks.is_empty(), "a goal picked up must see an empty queue");
            self.note(&format!("prioritized at stage {}", self.stage));
        }
        fn deprioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
            self.note(&format!("deprioritized at stage {}", self.stage));
        }
        fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
            self.note(&format!("process {}", last.name()));
            if last == TaskResult::Success {
                self.stage += 1;
            }
            if ctx.tasks.is_empty()
                && let Some(plan) = self.plan
            {
                let _ = ctx.tasks.push_back(plan);
            }
            GoalProgress::Working
        }
    }

    fn recorder(goal: GoalId, journal: &Arc<Mutex<Vec<String>>>, plan: Option<Task>) -> Box<dyn GoalExecutor> {
        Box::new(Recorder {
            goal,
            journal: journal.clone(),
            stage: 0,
            plan,
        })
    }

    fn dial(goal: GoalId, level: f32) -> (Box<dyn Routine>, Arc<Mutex<f32>>) {
        let level = Arc::new(Mutex::new(level));
        (
            Box::new(Dial {
                goal,
                level: level.clone(),
            }),
            level,
        )
    }

    fn room() -> World {
        World::new(Map::new(Size::new(12, 12), FLOOR))
    }

    #[test]
    fn a_fresh_brain_asks_its_goal_for_something_on_its_very_first_reaction() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let (wander, _) = dial(GoalId::Wander, 1.0);
        let wait = Task::Wait { seconds: 10.0 };
        let mut puppet = Puppet::new(
            Point::new(4, 4),
            Brain::new([wander], [recorder(GoalId::Wander, &journal, Some(wait))]),
        );
        let mut world = room();

        world.step(&mut puppet);

        assert_eq!(puppet.brain.top_goal(), GoalId::Wander);
        assert_eq!(puppet.brain.current_task(), Some(wait), "and started what it was given");
    }

    #[test]
    fn the_goal_losing_the_top_is_put_down_before_the_one_taking_it_is_picked_up() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let (wander, _) = dial(GoalId::Wander, 0.5);
        let (eat, hunger) = dial(GoalId::Eat, 0.0);
        let wait = Some(Task::Wait { seconds: 100.0 });
        let mut puppet = Puppet::new(
            Point::new(4, 4),
            Brain::new(
                [wander, eat],
                [recorder(GoalId::Wander, &journal, wait), recorder(GoalId::Eat, &journal, wait)],
            ),
        );
        let mut world = room();
        world.step(&mut puppet);
        journal.lock().unwrap().clear();

        *hunger.lock().unwrap() = 2.0;
        world.step(&mut puppet);

        let journal = journal.lock().unwrap();
        assert_eq!(
            journal[..3],
            [
                "wander deprioritized at stage 0".to_string(),
                "eat prioritized at stage 0".to_string(),
                "eat process in progress".to_string(),
            ]
        );
        assert_eq!(puppet.brain.top_goal(), GoalId::Eat);
    }

    #[test]
    fn a_goal_executor_keeps_its_stage_across_being_put_down_and_picked_up() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let (wander, _) = dial(GoalId::Wander, 0.5);
        let (eat, hunger) = dial(GoalId::Eat, 0.0);
        // A short wait, so wandering finishes tasks — and so gets further
        // along — while it is in charge.
        let short = Some(Task::Wait { seconds: 0.05 });
        let mut puppet = Puppet::new(
            Point::new(4, 4),
            Brain::new(
                [wander, eat],
                [recorder(GoalId::Wander, &journal, short), recorder(GoalId::Eat, &journal, short)],
            ),
        );
        let mut world = room();
        for _ in 0..40 {
            world.step(&mut puppet);
        }
        *hunger.lock().unwrap() = 2.0;
        world.step(&mut puppet);
        *hunger.lock().unwrap() = 0.0;
        world.step(&mut puppet);

        let journal = journal.lock().unwrap();
        let put_down = journal
            .iter()
            .find(|line| line.starts_with("wander deprioritized"))
            .expect("wander was put down");
        let picked_up = journal
            .iter()
            .rev()
            .find(|line| line.starts_with("wander prioritized"))
            .expect("wander was picked up again");
        let stage = |line: &str| line.rsplit(' ').next().unwrap().parse::<u32>().unwrap();
        assert!(stage(put_down) > 0, "wander should have got somewhere first: {put_down}");
        assert_eq!(stage(put_down), stage(picked_up));
    }

    #[test]
    fn the_top_executor_is_not_asked_again_while_its_action_is_running() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let (wander, _) = dial(GoalId::Wander, 1.0);
        let mut puppet = Puppet::new(
            Point::new(4, 4),
            Brain::new(
                [wander],
                [recorder(GoalId::Wander, &journal, Some(Task::Wait { seconds: 1.0 }))],
            ),
        );
        let mut world = room();
        // Just under a second of ticks at 64Hz: one wait, never finished.
        for _ in 0..60 {
            world.step(&mut puppet);
        }
        let processed = journal
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.contains("process"))
            .count();
        assert_eq!(processed, 1);
    }

    #[test]
    fn a_blocked_goal_hands_over_and_is_held_off() {
        struct Hopeless;
        impl GoalExecutor for Hopeless {
            fn goal(&self) -> GoalId {
                GoalId::Eat
            }
            fn process(&mut self, _: &mut GoalCtx<'_>, _: TaskResult) -> GoalProgress {
                GoalProgress::Blocked
            }
        }
        let journal = Arc::new(Mutex::new(Vec::new()));
        let (wander, _) = dial(GoalId::Wander, 0.1);
        let (eat, _) = dial(GoalId::Eat, 5.0);
        let mut puppet = Puppet::new(
            Point::new(4, 4),
            Brain::new(
                [wander, eat],
                [
                    recorder(GoalId::Wander, &journal, Some(Task::Wait { seconds: 100.0 })),
                    Box::new(Hopeless) as Box<dyn GoalExecutor>,
                ],
            ),
        );
        let mut world = room();
        world.step(&mut puppet);
        assert_eq!(puppet.brain.top_goal(), GoalId::Eat);
        world.step(&mut puppet);
        assert_eq!(puppet.brain.top_goal(), GoalId::Wander);
        assert!(puppet.brain.goals().cooldown(GoalId::Eat) > 0);
        // Still wanted: the routine's number is untouched.
        assert_eq!(puppet.brain.goals().priority(GoalId::Eat), 5.0);
    }

    /// Queues a walk to a random nearby cell whenever it has nothing queued,
    /// and counts the walks.
    struct Roamer {
        queued: Arc<Mutex<u64>>,
    }

    impl GoalExecutor for Roamer {
        fn goal(&self) -> GoalId {
            GoalId::Wander
        }
        fn process(&mut self, ctx: &mut GoalCtx<'_>, _last: TaskResult) -> GoalProgress {
            if ctx.tasks.is_empty() {
                let mut rng = crate::sim::rng::tick_rng(ctx.body.uid(), ctx.think.tick);
                let cell = Point::new(rng.random_range(1..11), rng.random_range(1..11));
                let _ = ctx.tasks.push_back(Task::MoveTo { cell });
                *self.queued.lock().unwrap() += 1;
            }
            GoalProgress::Working
        }
    }

    #[test]
    fn a_brain_never_asks_for_a_route_more_than_once_a_task() {
        // The performance test that matters. If `process` ends up running every
        // tick and its tasks asking for routes, the brain has reintroduced
        // unbudgeted per-agent A* — the named cause of the earlier prototype's
        // death. Counted rather than timed, so it cannot pass by being on a
        // fast machine.
        let queued = Arc::new(Mutex::new(0));
        let (wander, _) = dial(GoalId::Wander, 1.0);
        let mut puppet = Puppet::new(
            Point::new(4, 4),
            Brain::new(
                [wander],
                [Box::new(Roamer {
                    queued: queued.clone(),
                }) as Box<dyn GoalExecutor>],
            ),
        );
        let mut world = room();

        ROUTES_ASKED.with(|asked| asked.set(0));
        const TICKS: u64 = 1000;
        for _ in 0..TICKS {
            world.step(&mut puppet);
        }
        let routes = ROUTES_ASKED.with(|asked| asked.get());
        let tasks = *queued.lock().unwrap();

        assert!(tasks > 5, "it should have walked about: {tasks} tasks");
        assert!(routes <= tasks, "{routes} routes for {tasks} tasks");
        assert!(routes * 10 < TICKS, "{routes} routes in {TICKS} ticks");
    }

    #[test]
    fn a_walk_with_no_way_there_is_reported_to_the_goal_as_a_failure() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let (wander, _) = dial(GoalId::Wander, 1.0);
        // Off the map: never reachable.
        let nowhere = Some(Task::MoveTo {
            cell: Point::new(50, 50),
        });
        let mut puppet = Puppet::new(
            Point::new(4, 4),
            Brain::new([wander], [recorder(GoalId::Wander, &journal, nowhere)]),
        );
        let mut world = room();
        world.step(&mut puppet);
        world.step(&mut puppet);

        let journal = journal.lock().unwrap();
        assert_eq!(
            journal[..],
            ["wander prioritized at stage 0", "wander process in progress", "wander process failed"]
                .map(String::from)
        );
    }

    #[test]
    fn a_human_s_brain_says_what_it_is_doing() {
        let brain = Brain::human();
        let fields = brain.debug_fields();
        let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
        for wanted in ["goal", "priority", "task", "queue", "result", "action", "routines", "memory"] {
            assert!(names.contains(&wanted), "no {wanted} in {names:?}");
        }
        assert!(fields.iter().any(|(name, value)| *name == "routines" && value == "keep fed, stay busy"));
    }
}
