//! A unit's mind: [`Brain`], and the fixed pipeline that runs it.
//!
//! # Layers
//!
//! ```text
//! perception   what it can see                  (a stub, for now)
//! memory       what it remembers                (its own bed, so far)
//! routines     arrange the priority list        Need (fed, hydrated, ...), Sleep, StayBusy
//! goals        the list, and who is in charge   Idle, Wander, Eat, Drink, Relieve, Play, Sleep
//! tasks        the small steps a goal queued    MoveTo, TakeItem, ConsumeItem, UseToilet, ..., Wait
//! action       what a task is doing with body   pathfinding and timing only
//! ```
//!
//! Each layer only talks to the one below it. A routine does not queue tasks,
//! a goal does not ask for routes or change the world, a task does not choose
//! what comes next, an action does not decide anything — which is what lets
//! two goals take turns without either knowing about the other.
//!
//! # The pipeline
//!
//! It all runs in [`GameEntity::react`](super::GameEntity::react): the only
//! `&mut self` hook, and the one that runs after the whole crowd has moved.
//! `think` keeps what it had — walk the current route — so it stays arithmetic
//! and stays parallel.
//!
//! ```text
//! 0. perception.observe(..)                              stub, costs nothing
//! 1. every routine runs
//! 2.   ...and arranges the priority list
//! 3. if the goal on top changed: old.deprioritized() -> current task
//!      abandoned, walk halted -> queue cleared -> new.prioritized()
//! 4. ONLY IF the goal changed or no task is current: top.process(last result)
//! 5.   ...which manages the task queue
//! 6. the current task's executor runs (the front of the queue, if none is
//!      current), and writes the task's result
//! ```
//!
//! **A goal hears how a task went on the tick after it ended.** The task ends
//! in step 6; the next tick nothing is current, so step 4 asks the goal, which
//! decides — carry on, retry, replan — before step 6 starts anything else.
//! That is one tick between tasks, and it is the price of the goal getting a
//! say. "No task is current" also covers a goal that queued nothing, which is
//! asked again next tick; without that it would never be asked again.
//!
//! # Planning cadence, which is what matters
//!
//! A route is asked for in one place — a [`tasks::MoveTo`] starting its walk —
//! so the number of far searches is the number of walks started, and a walk
//! takes many ticks. A goal is only asked for more work when a task ends or
//! the goal in charge changes, and a goal that finds out expensively that it
//! cannot be done is held off ([`goal::BLOCKED_TICKS`], doubling). Per-agent,
//! per-tick planning is what killed the earlier prototype;
//! `a_goal_that_keeps_failing_does_not_ask_for_a_route_every_tick` is the test
//! that says this is not that.

pub mod action;
pub mod goal;
pub mod goals;
pub mod memory;
pub mod perception;
pub mod routine;
pub mod routines;
pub mod task;
pub mod tasks;

pub use action::{Action, ActionState};
pub use goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress, Goals};
pub use memory::{Memory, Recall};
pub use perception::Perception;
pub use routine::{Routine, RoutineCtx, RoutineExecutor};
pub use task::{Task, TaskCtx, TaskExecutor, TaskResult, Tasks};

use crate::map::Point;

use super::biology::Biology;
use super::entity::Think;
use super::inventory::Inventory;
use super::uid::Uid;
use super::walker::Walker;
use super::MoveOutcome;

use goals::{DrinkGoal, EatGoal, PlayGoal, RelieveGoal, SleepGoal, WanderGoal};
use routines::{BLADDER, BOREDOM, HUNGER, THIRST};

pub struct Brain {
    perception: Perception,
    memory: Memory,
    /// Run in this order every tick, as many as the brain was built with.
    ///
    /// Sized once, at construction, in the spawn pass — the one pass allowed to
    /// allocate — and never again: a boxed slice has no `push`, so the order is
    /// fixed for the unit's life, and an order that depended on anything at
    /// runtime is a determinism bug waiting for a thousand-tick test to find
    /// it. One allocation per unit however many routines it has, with their
    /// state inline; [`routine`] says why they are an enum and not boxes.
    routines: Box<[Routine]>,
    goals: Goals,
    /// One executor per [`GoalId`] this brain can pursue, `None` for one it
    /// cannot. **Owned for the entity's whole life** — which is the whole
    /// answer to how an executor keeps state across being put down.
    executors: [Option<Box<dyn GoalExecutor>>; GoalId::COUNT],
    tasks: Tasks,
    /// The task being carried out — off the queue, not yet ended.
    task: Option<Task>,
    /// What that task is doing with the body.
    action: Action,
    /// The task that ended since the goal was last asked, and what refused it
    /// if a body did — handed over in [`GoalCtx`] and then forgotten.
    finished: Option<Task>,
    blocked_by: Option<Uid>,
    /// How the last task to end went, kept for a debugger after the goal has
    /// been told.
    last_result: TaskResult,
}

impl Brain {
    /// A brain with these routines, run in the order given, and these
    /// executors, filed under the goal each pursues.
    ///
    /// Panics on two executors for one goal: a kind built wrong, found the
    /// first time one is spawned.
    pub fn new(
        routines: impl IntoIterator<Item = Routine>,
        executors: impl IntoIterator<Item = Box<dyn GoalExecutor>>,
    ) -> Brain {
        let mut brain = Brain {
            perception: Perception,
            memory: Memory::new(),
            routines: routines.into_iter().collect(),
            goals: Goals::new(),
            executors: [const { None }; GoalId::COUNT],
            tasks: Tasks::new(),
            action: Action::None,
            task: None,
            finished: None,
            blocked_by: None,
            last_result: TaskResult::InProgress,
        };
        for executor in executors {
            let slot = &mut brain.executors[executor.goal() as usize];
            assert!(slot.is_none(), "two executors for {:?}", executor.goal());
            *slot = Some(executor);
        }
        brain
    }

    /// A person: keeps fed, hydrated, comfortable, entertained and rested,
    /// stays busy; wanders, eats, drinks, uses the toilet, has a go on a
    /// computer, sleeps in a bed.
    pub fn human() -> Brain {
        Brain::new(
            [
                Routine::need(&HUNGER),
                Routine::need(&THIRST),
                Routine::need(&BLADDER),
                Routine::need(&BOREDOM),
                Routine::sleep(),
                Routine::stay_busy(),
            ],
            [
                Box::new(WanderGoal::new()) as Box<dyn GoalExecutor>,
                Box::new(EatGoal::new()),
                Box::new(DrinkGoal::new()),
                Box::new(RelieveGoal::new()),
                Box::new(PlayGoal::new()),
                Box::new(SleepGoal::new()),
            ],
        )
    }

    /// A dog: stays busy; wanders. Same machinery, fewer parts.
    pub fn dog() -> Brain {
        Brain::new(
            [Routine::stay_busy()],
            [Box::new(WanderGoal::new()) as Box<dyn GoalExecutor>],
        )
    }

    /// **The pipeline.** See the module docs for the steps and their order.
    ///
    /// `walk` is the body being driven; `biology` and `inventory` are `None`
    /// for a kind that has no needs or carries nothing.
    pub fn react(
        &mut self,
        ctx: &Think<'_>,
        outcome: MoveOutcome,
        walk: &mut Walker,
        mut biology: Option<&mut Biology>,
        mut inventory: Option<&mut Inventory>,
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
            task,
            action,
            finished,
            blocked_by,
            last_result,
        } = self;
        let body = *walk.body();

        // 0. Look around.
        perception.observe(ctx, &body);

        // 1, 2. Every routine arranges the list, from scratch.
        goals.clear_priorities();
        goals.tick_cooldowns();
        {
            let routine_ctx = RoutineCtx {
                think: ctx,
                body: &body,
                biology: biology.as_deref(),
            };
            for routine in routines.iter_mut() {
                routine.arrange(&routine_ctx, goals);
            }
        }

        let changed = goals.settle();
        {
            // Goals read the body and what it carries; only tasks, below,
            // change them.
            let mut goal_ctx = GoalCtx {
                think: ctx,
                body: &body,
                perception,
                biology: biology.as_deref(),
                memory,
                tasks,
                inventory: inventory.as_deref(),
                blocked_by: *blocked_by,
                finished: *finished,
            };

            // 3. Hand over, if the goal on top changed.
            if let Some((old, new)) = changed {
                if let Some(executor) = executors[old as usize].as_deref_mut() {
                    executor.deprioritized(&mut goal_ctx);
                }
                *task = None;
                *action = Action::None;
                walk.halt();
                goal_ctx.tasks.clear();
                // What ended belonged to the goal that queued it.
                goal_ctx.tasks.set_result(TaskResult::InProgress);
                goal_ctx.finished = None;
                goal_ctx.blocked_by = None;
                if let Some(executor) = executors[new as usize].as_deref_mut() {
                    executor.prioritized(&mut goal_ctx);
                }
            }

            // 4, 5. Ask the goal in charge — only if it just took charge, or
            // there is no task going on (which a task that ended last tick
            // leaves behind it).
            if changed.is_some() || task.is_none() {
                let top = goals.top();
                if let Some(executor) = executors[top as usize].as_deref_mut() {
                    let last = goal_ctx.tasks.result();
                    match executor.process(&mut goal_ctx, last) {
                        GoalProgress::Working => {
                            if last == TaskResult::Success {
                                goals.progressed(top);
                            }
                        }
                        GoalProgress::Achieved => {
                            goals.progressed(top);
                            goal_ctx.tasks.clear();
                        }
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
        }

        // 6. The current task's executor, which writes the task's result.
        if task.is_none() {
            *task = tasks.pop_front();
        }
        if let Some(current) = task.as_mut() {
            let result = current.execute(&mut TaskCtx {
                think: ctx,
                outcome,
                walk: &mut *walk,
                action: &mut *action,
                biology: biology.as_deref_mut(),
                inventory: inventory.as_deref_mut(),
            });
            tasks.set_result(result);
            if result.is_finished() {
                *last_result = result;
                *blocked_by = match result {
                    TaskResult::Failed => outcome.obstacle(),
                    _ => None,
                };
                *finished = task.take();
                *action = Action::None;
                walk.halt();
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

    /// Remember `bed` as this unit's own. What a goal that puts it to bed
    /// reads ([`memory::HOME_BED`]); handed out by the spawn pass and by
    /// nothing else, so two units are never given the same one.
    pub fn set_home(&mut self, bed: Point) {
        self.memory.remember(memory::HOME_BED, Recall::Cell(bed));
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
                // What the current task last wrote; between tasks, how the one
                // before ended.
                if self.task.is_some() {
                    self.tasks.result().name().to_string()
                } else {
                    self.last_result.name().to_string()
                },
            ),
            ("action", self.action.describe()),
            (
                "routines",
                self.routines
                    .iter()
                    .map(Routine::name)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ]);
        for routine in self.routines.iter() {
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
    use crate::map::{Map, Point, Size, FLOOR, WALL};
    use crate::sim::brain::routines::Dial;
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

    fn dial(goal: GoalId, level: f32) -> (Routine, Arc<Mutex<f32>>) {
        let level = Arc::new(Mutex::new(level));
        (
            Routine::Dial(Dial {
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
        let wait = Task::wait(10.0);
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
        let wait = Some(Task::wait(100.0));
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
        let short = Some(Task::wait(0.05));
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
                [recorder(GoalId::Wander, &journal, Some(Task::wait(1.0)))],
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
                    recorder(GoalId::Wander, &journal, Some(Task::wait(100.0))),
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

    /// Hears everything: the tick of every call, what it was handed and which
    /// task that was about — and, when put down, what it was handed then.
    /// Queues the tasks it was given, once.
    struct Listener {
        goal: GoalId,
        heard: Arc<Mutex<Vec<(u64, TaskResult, Option<Task>)>>>,
        put_down: Arc<Mutex<Option<(TaskResult, Option<Task>)>>>,
        plan: Vec<Task>,
    }

    impl GoalExecutor for Listener {
        fn goal(&self) -> GoalId {
            self.goal
        }
        fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
            *self.put_down.lock().unwrap() = Some((ctx.tasks.result(), ctx.finished));
        }
        fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
            self.heard.lock().unwrap().push((ctx.think.tick, last, ctx.finished));
            for task in self.plan.drain(..) {
                let _ = ctx.tasks.push_back(task);
            }
            GoalProgress::Working
        }
    }

    struct Listening {
        puppet: Puppet,
        world: World,
        heard: Arc<Mutex<Vec<(u64, TaskResult, Option<Task>)>>>,
        put_down: Arc<Mutex<Option<(TaskResult, Option<Task>)>>>,
        hunger: Arc<Mutex<f32>>,
    }

    const SHORT: Task = Task::wait(0.05);
    const LONG: Task = Task::wait(10.0);

    /// A wanderer that queues a short wait then a long one, and an eat goal
    /// that is not wanted until the test turns it up.
    fn listening() -> Listening {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let put_down = Arc::new(Mutex::new(None));
        let (wander, _) = dial(GoalId::Wander, 0.5);
        let (eat, hunger) = dial(GoalId::Eat, 0.0);
        let listener = |goal, plan| {
            Box::new(Listener {
                goal,
                heard: heard.clone(),
                put_down: put_down.clone(),
                plan,
            }) as Box<dyn GoalExecutor>
        };
        let brain = Brain::new(
            [wander, eat],
            [listener(GoalId::Wander, vec![SHORT, LONG]), listener(GoalId::Eat, vec![])],
        );
        Listening {
            puppet: Puppet::new(Point::new(4, 4), brain),
            world: room(),
            heard,
            put_down,
            hunger,
        }
    }

    impl Listening {
        /// Step until the short wait has ended; the tick it ended on.
        fn until_the_short_wait_ends(&mut self) -> u64 {
            for _ in 0..100 {
                self.world.step(&mut self.puppet);
                if self.puppet.brain.current_task().is_none()
                    && self.puppet.brain.last_result() == TaskResult::Success
                {
                    return self.world.tick;
                }
            }
            panic!("the short wait never ended");
        }
    }

    #[test]
    fn a_goal_hears_how_a_task_went_on_the_tick_after_it_ended() {
        let mut l = listening();
        let ended = l.until_the_short_wait_ends();
        {
            let heard = l.heard.lock().unwrap();
            assert!(
                heard.iter().all(|(tick, last, _)| *tick < ended || !last.is_finished()),
                "told on the very tick the task ended: {heard:?}"
            );
        }

        l.world.step(&mut l.puppet);
        assert_eq!(
            l.heard.lock().unwrap().last(),
            Some(&(ended + 1, TaskResult::Success, Some(SHORT)))
        );
    }

    #[test]
    fn the_next_task_does_not_start_before_the_goal_has_seen_the_last_one_end() {
        let mut l = listening();
        l.until_the_short_wait_ends();
        assert_eq!(l.puppet.brain.current_task(), None, "nothing starts on the tick one ends");
        assert_eq!(l.puppet.brain.tasks().len(), 1);

        l.world.step(&mut l.puppet);
        assert_eq!(l.puppet.brain.current_task(), Some(LONG));
    }

    #[test]
    fn a_goal_put_down_is_handed_the_result_it_had_not_read() {
        // The short wait ends; before the wanderer can be told, eating takes
        // over. Being put down is its last chance to hear.
        let mut l = listening();
        l.until_the_short_wait_ends();
        *l.hunger.lock().unwrap() = 2.0;
        l.world.step(&mut l.puppet);

        assert_eq!(l.puppet.brain.top_goal(), GoalId::Eat);
        assert_eq!(*l.put_down.lock().unwrap(), Some((TaskResult::Success, Some(SHORT))));
    }

    /// Walks to a random cell whenever nothing is queued, and gives up — the
    /// expensive miss — on a walk that failed with no body to blame. Keeps a
    /// count of the walks, and notes if it is ever asked for work while a
    /// task is still running.
    struct Roamer {
        queued: Arc<Mutex<u64>>,
        asked_mid_task: Arc<Mutex<bool>>,
    }

    impl GoalExecutor for Roamer {
        fn goal(&self) -> GoalId {
            GoalId::Wander
        }
        fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
            if last == TaskResult::Executing {
                *self.asked_mid_task.lock().unwrap() = true;
            }
            if last == TaskResult::Failed && ctx.blocked_by.is_none() {
                return GoalProgress::Blocked;
            }
            if ctx.tasks.is_empty() {
                let mut rng = crate::sim::rng::tick_rng(ctx.body.uid(), ctx.think.tick);
                let cell = Point::new(rng.random_range(1..11), rng.random_range(1..11));
                let _ = ctx.tasks.push_back(Task::move_to(cell));
                *self.queued.lock().unwrap() += 1;
            }
            GoalProgress::Working
        }
    }

    #[test]
    fn a_goal_that_keeps_failing_does_not_ask_for_a_route_every_tick() {
        // The performance test that matters. Half the room is walled off, so a
        // good share of the walks this goal picks have no way there — each a
        // flood of everything reachable before it knows. What stands between
        // that and a search every tick is the gate on step 4 and the back-off
        // on a blocked goal; this fails if either stops holding. Counted rather
        // than timed, so it cannot pass by being on a fast machine.
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        for y in 0..12 {
            map.set_terrain(Point::new(6, y), WALL);
        }
        let queued = Arc::new(Mutex::new(0));
        let asked_mid_task = Arc::new(Mutex::new(false));
        let (wander, _) = dial(GoalId::Wander, 1.0);
        let mut puppet = Puppet::new(
            Point::new(2, 4),
            Brain::new(
                [wander],
                [Box::new(Roamer {
                    queued: queued.clone(),
                    asked_mid_task: asked_mid_task.clone(),
                }) as Box<dyn GoalExecutor>],
            ),
        );
        let mut world = World::new(map);

        ROUTES_ASKED.with(|asked| asked.set(0));
        const TICKS: u64 = 1000;
        for _ in 0..TICKS {
            world.step(&mut puppet);
        }
        let routes = ROUTES_ASKED.with(|asked| asked.get());

        assert!(*queued.lock().unwrap() > 3, "it should have tried to walk about");
        assert!(!*asked_mid_task.lock().unwrap(), "a goal was asked for work mid-task");
        assert!(routes * 10 < TICKS, "{routes} routes in {TICKS} ticks");
    }

    #[test]
    fn a_walk_with_no_way_there_is_reported_to_the_goal_as_a_failure() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let (wander, _) = dial(GoalId::Wander, 1.0);
        // Off the map: never reachable.
        let nowhere = Some(Task::move_to(Point::new(50, 50)));
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
    fn a_brain_runs_every_routine_it_was_given_in_order_however_many_there_are() {
        // Far more than any fixed cap there used to be. Each dial raises
        // wandering a little higher than the one before, so the priority that
        // comes out is the last routine's: it only gets there if all of them
        // ran.
        const MANY: usize = 64;
        let dials: Vec<Routine> = (1..=MANY).map(|i| dial(GoalId::Wander, i as f32).0).collect();
        let mut puppet = Puppet::new(Point::new(4, 4), Brain::new(dials, []));
        let mut world = room();
        world.step(&mut puppet);

        assert_eq!(puppet.brain.goals().priority(GoalId::Wander), MANY as f32);
        let listed = puppet
            .brain
            .debug_fields()
            .into_iter()
            .find(|(name, _)| *name == "routines")
            .map(|(_, value)| value.split(", ").count());
        assert_eq!(listed, Some(MANY));
    }

    #[test]
    fn a_brain_with_no_routines_wants_nothing() {
        let mut puppet = Puppet::new(Point::new(4, 4), Brain::new([], []));
        room().step(&mut puppet);
        assert_eq!(puppet.brain.top_goal(), GoalId::Idle);
    }

    #[test]
    fn a_human_s_brain_says_what_it_is_doing() {
        let brain = Brain::human();
        let fields = brain.debug_fields();
        let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
        for wanted in ["goal", "priority", "task", "queue", "result", "action", "routines", "memory"] {
            assert!(names.contains(&wanted), "no {wanted} in {names:?}");
        }
        assert!(fields.iter().any(|(name, value)| *name == "routines" && value == "keep fed, keep hydrated, stay comfortable, keep entertained, get some sleep, stay busy"));
    }
}
