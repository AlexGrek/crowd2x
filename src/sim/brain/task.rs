//! Tasks: the small steps a goal breaks itself into, the queue they wait in,
//! and [`TaskExecutor`] — what carries one out.
//!
//! A goal *chooses* tasks ([`super::goal`]); a task *manages* one small thing
//! — walk to that cell, take food out of that fridge if already standing
//! beside it, eat what is in hand — by starting an [`Action`], watching it,
//! and applying what finishing it means. **A task writes the [`TaskResult`]**,
//! and the goal that queued it reads it on the tick after, to decide whether
//! to carry on, retry or replan.
//!
//! # Executors without a box
//!
//! Every task is its own executor struct ([`super::tasks`]), and [`Task`] is
//! the enum over them, dispatched by `match`. A `Box<dyn TaskExecutor>` per
//! queued task would be an allocation every time a goal plans, inside the pass
//! that may not allocate; an enum of `Copy` structs lives inline in the queue.
//!
//! # Adding a task
//!
//! A file in [`super::tasks`] with the struct, its [`TaskExecutor`] impl, its
//! `Task::` constructor and how it is saved — [`TimedTask`] for a task that is
//! a place and a time, which is a name and four one-line methods, or
//! [`SavedTask`] by hand — and its name in the [`tasks!`] list in
//! `tasks/mod.rs`, which writes the enum and every `match` over it. Nothing in
//! the save schema: a timed task is saved under its own name.

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};
use crate::sim::biology::Biology;
use crate::sim::entity::Think;
use crate::sim::inventory::Inventory;
use crate::sim::item::ItemKind;
use crate::sim::walker::Walker;
use crate::sim::{Effect, MoveOutcome};

use super::action::{Action, ActionState};
pub use super::tasks::Task;

/// What a task may touch while it runs: the world to read, the body it moves,
/// the action it drives, and the parts of its unit a task can change.
pub struct TaskCtx<'a> {
    pub think: &'a Think<'a>,
    /// What became of this tick's step, for a walk to repair its route by.
    pub outcome: MoveOutcome,
    pub walk: &'a mut Walker,
    /// The action this task started, or [`Action::None`] before it has.
    pub action: &'a mut Action,
    /// The body. A task never writes a stat: it tells the body what happened
    /// ([`Biology::handle`]) and the body's processes decide what that means.
    /// `None` for a kind that has no needs.
    pub biology: Option<&'a mut Biology>,
    /// What it is carrying: the hand a task takes into and consumes from, and
    /// what it has stowed. `None` for a kind that carries nothing.
    ///
    /// The one part of a unit a task may put things into and take things out
    /// of — a goal only gets to read it ([`super::goal::GoalCtx`]), which is
    /// what makes food arrive in a hand when a `TakeItem` *finishes* rather
    /// than when a goal hears that it did.
    pub inventory: Option<&'a mut Inventory>,
    /// What this task asks of the world beyond its own unit — a fridge's
    /// door, a door's leaf, a piece off a heap of scrap, a packful into a
    /// scrap bin. Starts each tick as [`Effect::None`]; a task that wants
    /// to ask for something writes it once, the tick its action finishes, and
    /// [`super::super::world_step`](crate::sim::world_step) is what actually
    /// grants it, sequentially, after every entity has reacted.
    pub effect: &'a mut Effect,
}

impl TaskCtx<'_> {
    /// Advance the running action by one tick — and, walking, ask for the
    /// door ahead to open, or to stay open while it is crossed. Here rather
    /// than in each task that walks, so every walk does it: the walker stands
    /// still at a shut door (`Walker::think`), and this is the other half,
    /// the ask, which only the reaction round may make.
    pub fn advance_action(&mut self) -> ActionState {
        let state = self.action.advance(self.think, self.outcome, self.walk);
        if matches!(self.action, Action::Move { .. })
            && *self.effect == Effect::None
            && let Some(at) = self.walk.door_ahead(self.think)
        {
            *self.effect = Effect::Door {
                at,
                key: self.walk.body().home(),
            };
        }
        state
    }

    /// Where the unit is standing.
    pub fn here(&self) -> Point {
        self.walk.body().center_position()
    }
}

/// Carries out one task, a tick at a time.
pub trait TaskExecutor {
    /// One tick of the task. With no action running, check what has to be
    /// true and start one — or fail; with one running, advance it, and when it
    /// finishes, apply what that means.
    ///
    /// Called every tick the task is current, so what it checks it checks
    /// every tick: a precondition that was true when the action started is
    /// not assumed to still be.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult;

    /// It is being dropped half done — another goal took charge — and this is
    /// its one chance to tell the body what it **already** did. `action` is
    /// the action as it stood.
    ///
    /// Most tasks owe nothing: half a meal is not eaten and a fridge half
    /// opened is not open, so the default does nothing. The exception is one
    /// whose worth is the time spent rather than the thing finished — an hour
    /// of sleep cut short at forty minutes was forty minutes of sleep.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        let _ = (action, biology);
    }

    /// How it reads in a debug view.
    fn describe(&self) -> String;
}

/// How a task is kept in a save, and found in one again.
pub trait SavedTask: Sized {
    fn save(&self) -> pb::TaskKind;

    /// This task, if `saved` is one: `None` for another task's, an error for
    /// one of its own that is broken.
    fn restore(saved: &pb::TaskKind) -> Option<Result<Self, Invalid>>;
}

/// A task that is a place and a time — the toilet, the bed, the store, the
/// furnace — and so saves itself as both under its [`TimedTask::NAME`]
/// ([`pb::TaskKind::Named`]), with no schema of its own.
pub trait TimedTask: Sized {
    /// What a save calls it: a name no other task has, never changed once a
    /// save could hold it.
    const NAME: &'static str;

    fn new(cell: Point, seconds: f32) -> Self;
    fn cell(&self) -> Point;
    fn seconds(&self) -> f32;

    /// Where the builds before the names saved it, one tag a task
    /// (`pb::TaskKind`, tags 3-19). A task added since has none.
    fn legacy(saved: &pb::TaskKind) -> Option<&pb::Timed> {
        let _ = saved;
        None
    }
}

impl<T: TimedTask> SavedTask for T {
    fn save(&self) -> pb::TaskKind {
        named(T::NAME, Some(self.cell()), self.seconds())
    }

    fn restore(saved: &pb::TaskKind) -> Option<Result<T, Invalid>> {
        let (cell, seconds) = match T::legacy(saved) {
            Some(task) => (task.cell, task.seconds),
            None => read_named(saved, T::NAME)?,
        };
        Some(cell_and_seconds(cell, seconds, T::NAME).map(|(cell, seconds)| T::new(cell, seconds)))
    }
}

/// A task saved under `name`: [`pb::TaskKind::Named`].
pub(crate) fn named(name: &str, cell: Option<Point>, seconds: f32) -> pb::TaskKind {
    pb::TaskKind::Named(pb::NamedTask { name: name.to_string(), cell: cell.map(Into::into), seconds })
}

/// The cell and the seconds of `saved`, if it is the task called `name`.
pub(crate) fn read_named(saved: &pb::TaskKind, name: &str) -> Option<(Option<pb::Cell>, f32)> {
    match saved {
        pb::TaskKind::Named(task) if task.name == name => Some((task.cell, task.seconds)),
        _ => None,
    }
}

/// A saved length of time, refused if it is not one.
pub(crate) fn seconds(seconds: f32) -> Result<f32, Invalid> {
    if seconds.is_finite() && seconds >= 0.0 {
        Ok(seconds)
    } else {
        invalid(format!("task: {seconds} seconds long"))
    }
}

/// A saved item, by its name.
pub(crate) fn item(name: &str) -> Result<ItemKind, Invalid> {
    ItemKind::from_name(name).ok_or_else(|| Invalid::new(format!("task: no such item {name:?}")))
}

/// A saved cell and length of time, both of which there have to be.
pub(crate) fn cell_and_seconds(cell: Option<pb::Cell>, time: f32, what: &str) -> Result<(Point, f32), Invalid> {
    Ok((pb::cell(cell, what)?, seconds(time)?))
}

/// Writes [`Task`] from the list of task structs: the enum, a variant each
/// named after its struct, and the `match`es over it — execute, interrupted,
/// describe, save and restore. Used once, in `tasks/mod.rs`.
macro_rules! tasks {
    ($($task:ident),* $(,)?) => {
        /// One step of a plan.
        #[derive(Clone, Copy, PartialEq, Debug)]
        pub enum Task {
            $($task($task),)*
        }

        impl Task {
            /// Run this task's executor for a tick.
            pub fn execute(&mut self, ctx: &mut $crate::sim::brain::task::TaskCtx<'_>) -> $crate::sim::brain::task::TaskResult {
                use $crate::sim::brain::task::TaskExecutor;
                match self {
                    $(Task::$task(task) => task.execute(ctx),)*
                }
            }

            /// Tell this task's executor it is being dropped half done. See
            /// [`TaskExecutor::interrupted`]($crate::sim::brain::task::TaskExecutor::interrupted).
            pub fn interrupted(&mut self, action: &$crate::sim::brain::action::Action, biology: Option<&mut $crate::sim::biology::Biology>) {
                use $crate::sim::brain::task::TaskExecutor;
                match self {
                    $(Task::$task(task) => task.interrupted(action, biology),)*
                }
            }

            pub fn describe(&self) -> String {
                use $crate::sim::brain::task::TaskExecutor;
                match self {
                    $(Task::$task(task) => task.describe(),)*
                }
            }

            pub(crate) fn save(&self) -> $crate::save::pb::Task {
                use $crate::sim::brain::task::SavedTask;
                let kind = match self {
                    $(Task::$task(task) => task.save(),)*
                };
                $crate::save::pb::Task { kind: Some(kind) }
            }

            pub(crate) fn restore(saved: &$crate::save::pb::Task) -> Result<Task, $crate::save::Invalid> {
                use $crate::sim::brain::task::SavedTask;
                let kind = saved.kind.as_ref().ok_or_else(|| $crate::save::Invalid::new("a task of no kind"))?;
                $(
                    if let Some(task) = <$task as SavedTask>::restore(kind) {
                        return task.map(Task::$task);
                    }
                )*
                match kind {
                    $crate::save::pb::TaskKind::Named(task) => $crate::save::invalid(format!("no task called {:?}", task.name)),
                    _ => $crate::save::invalid("a task this build does not know"),
                }
            }
        }
    };
}

pub(crate) use tasks;

/// What a task says about itself, and what the goal that queued it reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TaskResult {
    /// The task has not got an action going. Also what a goal is handed when
    /// no task has ended since it was last asked.
    #[default]
    InProgress,
    /// Its action is running.
    Executing,
    Failed,
    Success,
}

impl TaskResult {
    pub(crate) fn save(self) -> i32 {
        (match self {
            TaskResult::InProgress => pb::TaskResult::InProgress,
            TaskResult::Executing => pb::TaskResult::Executing,
            TaskResult::Failed => pb::TaskResult::Failed,
            TaskResult::Success => pb::TaskResult::Success,
        }) as i32
    }

    pub(crate) fn restore(raw: i32) -> Result<TaskResult, Invalid> {
        Ok(match pb::known::<pb::TaskResult>(raw, "task result")? {
            pb::TaskResult::InProgress => TaskResult::InProgress,
            pb::TaskResult::Executing => TaskResult::Executing,
            pb::TaskResult::Failed => TaskResult::Failed,
            pb::TaskResult::Success => TaskResult::Success,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            TaskResult::InProgress => "in progress",
            TaskResult::Executing => "executing",
            TaskResult::Failed => "failed",
            TaskResult::Success => "success",
        }
    }

    /// Whether this is the end of a task rather than the middle of one.
    pub const fn is_finished(self) -> bool {
        matches!(self, TaskResult::Failed | TaskResult::Success)
    }

    /// The result an action's state makes, for a task whose action is all
    /// there is to it.
    pub const fn of(state: ActionState) -> TaskResult {
        match state {
            ActionState::Running => TaskResult::Executing,
            ActionState::Finished => TaskResult::Success,
            ActionState::Failed => TaskResult::Failed,
        }
    }
}

/// Double-ended, fixed capacity, inline.
///
/// Planning forward pushes on the back ("then eat it"); discovering a
/// prerequisite pushes on the front ("get there first"), which is also what
/// resuming an interrupted goal is. A push into a full queue is **refused**,
/// not grown — nothing in a tick may allocate, and a goal that wants more than
/// [`Tasks::CAPACITY`] steps queued at once is planning too far ahead of a
/// world that will have changed by the time it gets there.
#[derive(Clone, Copy, Debug)]
pub struct Tasks {
    slots: [Option<Task>; Tasks::CAPACITY],
    /// Index of the front task. Only meaningful while `len > 0`.
    head: u8,
    len: u8,
    result: TaskResult,
}

impl Default for Tasks {
    fn default() -> Tasks {
        Tasks::new()
    }
}

impl Tasks {
    pub const CAPACITY: usize = 8;

    pub const fn new() -> Tasks {
        Tasks {
            slots: [None; Tasks::CAPACITY],
            head: 0,
            len: 0,
            result: TaskResult::InProgress,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.len() == Tasks::CAPACITY
    }

    /// The slot `offset` places behind the front.
    fn index(&self, offset: usize) -> usize {
        (self.head as usize + offset) % Tasks::CAPACITY
    }

    /// The next task to run.
    pub fn front(&self) -> Option<Task> {
        if self.is_empty() {
            None
        } else {
            self.slots[self.head as usize]
        }
    }

    /// The last task queued.
    pub fn back(&self) -> Option<Task> {
        if self.is_empty() {
            None
        } else {
            self.slots[self.index(self.len() - 1)]
        }
    }

    /// Queue a task to run *next*. Returns `false`, and changes nothing, when
    /// the queue is full.
    #[must_use = "a full queue refuses the task"]
    pub fn push_front(&mut self, task: Task) -> bool {
        if self.is_full() {
            return false;
        }
        self.head = ((self.head as usize + Tasks::CAPACITY - 1) % Tasks::CAPACITY) as u8;
        self.slots[self.head as usize] = Some(task);
        self.len += 1;
        true
    }

    /// Queue a task to run *after* everything already queued. Returns `false`,
    /// and changes nothing, when the queue is full.
    #[must_use = "a full queue refuses the task"]
    pub fn push_back(&mut self, task: Task) -> bool {
        if self.is_full() {
            return false;
        }
        let at = self.index(self.len());
        self.slots[at] = Some(task);
        self.len += 1;
        true
    }

    pub fn pop_front(&mut self) -> Option<Task> {
        if self.is_empty() {
            return None;
        }
        let task = self.slots[self.head as usize].take();
        self.head = ((self.head as usize + 1) % Tasks::CAPACITY) as u8;
        self.len -= 1;
        task
    }

    /// Drop every queued task. The result is left alone: it is about a task
    /// that already ran, and clearing the queue does not un-run it.
    pub fn clear(&mut self) {
        self.slots = [None; Tasks::CAPACITY];
        self.head = 0;
        self.len = 0;
    }

    /// Front to back.
    pub fn iter(&self) -> impl Iterator<Item = Task> + '_ {
        (0..self.len()).filter_map(|offset| self.slots[self.index(offset)])
    }

    /// What the most recent task wrote.
    pub fn result(&self) -> TaskResult {
        self.result
    }

    // --- saving ---

    /// The queue front to back, and the result. Where in the ring it sits is
    /// not kept: it changes nothing the queue does.
    pub(crate) fn save(&self) -> (Vec<pb::Task>, i32) {
        let Tasks { slots: _, head: _, len: _, result } = self;
        (self.iter().map(|task| task.save()).collect(), result.save())
    }

    pub(crate) fn restore(queue: &[pb::Task], result: i32) -> Result<Tasks, Invalid> {
        let mut tasks = Tasks::new();
        for task in queue {
            if !tasks.push_back(Task::restore(task)?) {
                return invalid(format!("{} tasks queued, more than {}", queue.len(), Tasks::CAPACITY));
            }
        }
        tasks.result = TaskResult::restore(result)?;
        Ok(tasks)
    }

    /// Where a task's result is written. The brain passes on what the task
    /// returned and nothing else; an executor only reads it.
    pub(super) fn set_result(&mut self, result: TaskResult) {
        self.result = result;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(seconds: f32) -> Task {
        Task::wait(seconds)
    }

    #[test]
    fn tasks_pushed_on_the_back_run_in_the_order_they_were_queued() {
        let mut tasks = Tasks::new();
        assert!(tasks.push_back(wait(1.0)));
        assert!(tasks.push_back(wait(2.0)));
        assert!(tasks.push_back(wait(3.0)));

        assert_eq!(tasks.pop_front(), Some(wait(1.0)));
        assert_eq!(tasks.pop_front(), Some(wait(2.0)));
        assert_eq!(tasks.pop_front(), Some(wait(3.0)));
        assert_eq!(tasks.pop_front(), None);
    }

    #[test]
    fn a_task_pushed_on_the_front_is_the_next_one_to_run() {
        // A prerequisite discovered after planning — "get there first" — has
        // to jump the queue, which is the reason the queue is double-ended.
        let mut tasks = Tasks::new();
        assert!(tasks.push_back(wait(2.0)));
        assert!(tasks.push_front(wait(1.0)));

        assert_eq!(tasks.front(), Some(wait(1.0)));
        assert_eq!(tasks.back(), Some(wait(2.0)));
        assert_eq!(tasks.iter().collect::<Vec<_>>(), [wait(1.0), wait(2.0)]);
    }

    #[test]
    fn a_full_queue_refuses_a_task_rather_than_growing() {
        let mut tasks = Tasks::new();
        for i in 0..Tasks::CAPACITY {
            assert!(tasks.push_back(wait(i as f32)));
        }
        assert!(!tasks.push_back(wait(99.0)));
        assert!(!tasks.push_front(wait(99.0)));
        assert_eq!(tasks.len(), Tasks::CAPACITY);
        // The refused tasks are nowhere in it.
        assert!(tasks.iter().all(|task| task != wait(99.0)));
        assert_eq!(tasks.front(), Some(wait(0.0)));
    }

    #[test]
    fn the_queue_wraps_round_its_slots_without_losing_order() {
        // Pushing on the front of an empty queue starts at the last slot, so
        // over enough rounds every push and pop crosses the seam.
        let mut tasks = Tasks::new();
        for round in 0..50 {
            let r = round as f32;
            assert!(tasks.push_front(wait(r)));
            assert!(tasks.push_back(wait(r + 0.5)));
            assert!(tasks.push_back(wait(r + 0.75)));
            assert_eq!(tasks.pop_front(), Some(wait(r)));
            assert_eq!(tasks.pop_front(), Some(wait(r + 0.5)));
            assert_eq!(tasks.pop_front(), Some(wait(r + 0.75)));
            assert!(tasks.is_empty());
        }
    }

    #[test]
    fn clearing_empties_the_queue_but_keeps_how_the_last_task_went() {
        let mut tasks = Tasks::new();
        assert!(tasks.push_back(wait(1.0)));
        tasks.set_result(TaskResult::Success);
        tasks.clear();
        assert!(tasks.is_empty());
        assert_eq!(tasks.front(), None);
        assert_eq!(tasks.result(), TaskResult::Success);
    }

    /// One of every task there is.
    fn one_of_each() -> Vec<Task> {
        let at = Point::new(3, -4);
        vec![
            Task::move_to(at),
            Task::move_beside(at),
            Task::take(at, ItemKind::Water, 2.0),
            Task::consume(ItemKind::Food, 3.0),
            Task::use_toilet(at, 4.0),
            Task::use_computer(at, 5.0),
            Task::watch_tv(at, 6.0),
            Task::sleep(at, 7.0),
            Task::wait(8.0),
            Task::open_fridge(at, 9.0),
            Task::close_fridge(at, 10.0),
            Task::salvage(at, 11.0),
            Task::salvage_for_work(at, 12.0),
            Task::work(at, 13.0),
            Task::keep_watch(at, 14.0),
            Task::deposit(at, 15.0),
            Task::take_body(at, 16.0),
            Task::cremate(at, 17.0),
            Task::withdraw(at, 18.0),
            Task::smelt(at, 19.0),
            Task::cook(at, 19.0),
            Task::use_arcade(at, 23.0),
            Task::sow(at, 20.0),
            Task::tend(at, 21.0),
            Task::harvest(at, 22.0),
            Task::work_at(crate::sim::feature::FeatureKind::ControlPanel, at, 23.0),
        ]
    }

    /// Every task comes back from a save as itself, and no two tasks are
    /// saved alike — which, for the ones saved by name, is no two names the
    /// same.
    #[test]
    fn every_task_comes_back_from_a_save_as_itself() {
        let tasks = one_of_each();
        for (index, task) in tasks.iter().enumerate() {
            let saved = task.save();
            assert_eq!(Task::restore(&saved).unwrap(), *task, "{task:?} as {saved:?}");
            for other in &tasks[..index] {
                let other = other.save();
                if let (Some(pb::TaskKind::Named(a)), Some(pb::TaskKind::Named(b))) = (&saved.kind, &other.kind) {
                    assert_ne!(a.name, b.name, "two tasks saved by one name");
                }
            }
        }
        let unknown = pb::Task { kind: Some(named("juggle", None, 1.0)) };
        assert!(Task::restore(&unknown).is_err(), "a name this build has no task for");
    }

    /// What the builds before the names saved, a tag a task, still loads as
    /// the task it was.
    #[test]
    fn a_task_saved_under_its_old_tag_still_loads() {
        let at = Point::new(3, -4);
        let timed = |seconds: f32| pb::Timed { cell: Some(at.into()), seconds };
        let old = |kind: pb::TaskKind| Task::restore(&pb::Task { kind: Some(kind) }).unwrap();
        assert_eq!(old(pb::TaskKind::UseToilet(timed(4.0))), Task::use_toilet(at, 4.0));
        assert_eq!(old(pb::TaskKind::UseComputer(timed(5.0))), Task::use_computer(at, 5.0));
        assert_eq!(old(pb::TaskKind::Sleep(timed(7.0))), Task::sleep(at, 7.0));
        assert_eq!(old(pb::TaskKind::Wait(pb::Timed { cell: None, seconds: 8.0 })), Task::wait(8.0));
        assert_eq!(old(pb::TaskKind::OpenFridge(timed(9.0))), Task::open_fridge(at, 9.0));
        assert_eq!(old(pb::TaskKind::CloseFridge(timed(10.0))), Task::close_fridge(at, 10.0));
        assert_eq!(old(pb::TaskKind::Salvage(timed(11.0))), Task::salvage(at, 11.0));
        assert_eq!(old(pb::TaskKind::Work(timed(13.0))), Task::work(at, 13.0));
        assert_eq!(old(pb::TaskKind::Deposit(timed(15.0))), Task::deposit(at, 15.0));
        assert_eq!(old(pb::TaskKind::SalvageForWork(timed(12.0))), Task::salvage_for_work(at, 12.0));
        assert_eq!(old(pb::TaskKind::KeepWatch(timed(14.0))), Task::keep_watch(at, 14.0));
        assert_eq!(old(pb::TaskKind::TakeBody(timed(16.0))), Task::take_body(at, 16.0));
        assert_eq!(old(pb::TaskKind::Cremate(timed(17.0))), Task::cremate(at, 17.0));
        assert_eq!(old(pb::TaskKind::WatchTv(timed(6.0))), Task::watch_tv(at, 6.0));
        assert_eq!(old(pb::TaskKind::Withdraw(timed(18.0))), Task::withdraw(at, 18.0));
        assert_eq!(old(pb::TaskKind::Smelt(timed(19.0))), Task::smelt(at, 19.0));
    }
}
