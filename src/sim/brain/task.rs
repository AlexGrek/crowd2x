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
//! Adding a task is a struct, an [`TaskExecutor`] impl and a variant here.

use crate::map::Point;
use crate::sim::biology::Biology;
use crate::sim::entity::Think;
use crate::sim::inventory::Inventory;
use crate::sim::item::ItemKind;
use crate::sim::walker::Walker;
use crate::sim::MoveOutcome;

use super::action::{Action, ActionState};
use super::tasks::{ConsumeItem, MoveTo, TakeItem, UseComputer, UseToilet, Wait};

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
}

impl TaskCtx<'_> {
    /// Advance the running action by one tick.
    pub fn advance_action(&mut self) -> ActionState {
        self.action.advance(self.think, self.outcome, self.walk)
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

    /// How it reads in a debug view.
    fn describe(&self) -> String;
}

/// One step of a plan.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Task {
    MoveTo(MoveTo),
    TakeItem(TakeItem),
    ConsumeItem(ConsumeItem),
    UseToilet(UseToilet),
    UseComputer(UseComputer),
    Wait(Wait),
}

impl Task {
    /// Walk until standing in the middle of `cell`.
    pub const fn move_to(cell: Point) -> Task {
        Task::MoveTo(MoveTo { cell })
    }

    /// Take `item` out of whatever is in `from`, from a cell beside it.
    pub const fn take(from: Point, item: ItemKind, seconds: f32) -> Task {
        Task::TakeItem(TakeItem {
            from,
            item,
            seconds,
        })
    }

    /// Use up the `item` in hand.
    pub const fn consume(item: ItemKind, seconds: f32) -> Task {
        Task::ConsumeItem(ConsumeItem { item, seconds })
    }

    /// Use the toilet in `toilet`, from a cell beside it.
    pub const fn use_toilet(toilet: Point, seconds: f32) -> Task {
        Task::UseToilet(UseToilet { toilet, seconds })
    }

    /// Have a go on the computer in `computer`, from a cell beside it.
    pub const fn use_computer(computer: Point, seconds: f32) -> Task {
        Task::UseComputer(UseComputer { computer, seconds })
    }

    /// Stand still for a while.
    pub const fn wait(seconds: f32) -> Task {
        Task::Wait(Wait { seconds })
    }

    /// Run this task's executor for a tick.
    pub fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        match self {
            Task::MoveTo(task) => task.execute(ctx),
            Task::TakeItem(task) => task.execute(ctx),
            Task::ConsumeItem(task) => task.execute(ctx),
            Task::UseToilet(task) => task.execute(ctx),
            Task::UseComputer(task) => task.execute(ctx),
            Task::Wait(task) => task.execute(ctx),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Task::MoveTo(task) => task.describe(),
            Task::TakeItem(task) => task.describe(),
            Task::ConsumeItem(task) => task.describe(),
            Task::UseToilet(task) => task.describe(),
            Task::UseComputer(task) => task.describe(),
            Task::Wait(task) => task.describe(),
        }
    }
}

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
}
