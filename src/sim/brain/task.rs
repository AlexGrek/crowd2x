//! Tasks: the steps a goal breaks itself into, and the queue they wait in.
//!
//! A task is a *decision already made* — walk to this cell, use that fridge,
//! wait this long. Choosing them is a goal's job ([`super::goal`]); carrying
//! one out is an action's ([`super::action`]). This module is only the
//! vocabulary between the two and the queue that holds it.

use crate::map::Point;

/// One step of a plan.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Task {
    /// Walk until standing in `cell`.
    MoveTo { cell: Point },
    /// Do something to whatever is in `cell`, from a cell beside it.
    ///
    /// **Fails rather than walking** — the walking is a task that should have
    /// been queued in front of this one. An interaction that quietly walked
    /// would hide a goal that forgot to plan the route, and it would be a
    /// second place a route gets asked for.
    Interact { cell: Point, seconds: f32 },
    /// Stand still for a while.
    Wait { seconds: f32 },
}

impl Task {
    /// How it reads in a debug view.
    pub fn describe(&self) -> String {
        match self {
            Task::MoveTo { cell } => format!("move to {}, {}", cell.x, cell.y),
            Task::Interact { cell, seconds } => {
                format!("use {}, {} for {seconds:.1}s", cell.x, cell.y)
            }
            Task::Wait { seconds } => format!("wait {seconds:.1}s"),
        }
    }
}

/// What became of the most recent task.
///
/// Handed to [`super::goal::GoalExecutor::process`] as `last`, which is how a
/// goal learns that the step it queued is over and how it went.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TaskResult {
    /// Nothing has finished since the executor was last told anything.
    #[default]
    InProgress,
    /// An action is running.
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

    pub fn result(&self) -> TaskResult {
        self.result
    }

    /// Only the brain says how a task went — an executor reads it.
    pub(super) fn set_result(&mut self, result: TaskResult) {
        self.result = result;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(seconds: f32) -> Task {
        Task::Wait { seconds }
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
