//! Goals: what a unit wants, how badly, and who pursues it.
//!
//! Two halves, kept apart on purpose:
//!
//! * [`Goals`] is the **priority list** — a number per goal, arranged every
//!   tick by the routines ([`super::routine`]), and the one that comes out on
//!   top is the goal in charge. It knows nothing about what pursuing a goal
//!   involves.
//! * A [`GoalExecutor`] is **how** one goal is pursued: it turns "eat" into
//!   "walk to the fridge, take food, eat it", one queued [`Task`] at a time. It
//!   knows nothing about whether its goal should be the one in charge.
//!
//! [`Task`]: super::task::Task

use crate::sim::biology::Biology;
use crate::sim::entity::{Body, Think};
use crate::sim::inventory::Inventory;
use crate::sim::uid::Uid;

use super::memory::Memory;
use super::perception::Perception;
use super::task::{Task, TaskResult, Tasks};

/// Every goal a brain can hold.
///
/// An enum over a fixed array rather than a map of strings: a typo in a
/// variant is a compile error where a typo in a string is a runtime one, the
/// discriminants are the storage order (as `map::ObjectLayer`'s are), and
/// iterating in declaration order is what the determinism tests depend on.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum GoalId {
    /// Nothing to do. The goal in charge when no other goal is wanted at all,
    /// which is why it has no executor anywhere and cannot be put on a
    /// cooldown.
    Idle = 0,
    Wander = 1,
    Eat = 2,
    Drink = 3,
    Relieve = 4,
}

impl GoalId {
    pub const COUNT: usize = 5;
    pub const ALL: [GoalId; GoalId::COUNT] =
        [GoalId::Idle, GoalId::Wander, GoalId::Eat, GoalId::Drink, GoalId::Relieve];

    pub const fn name(self) -> &'static str {
        match self {
            GoalId::Idle => "idle",
            GoalId::Wander => "wander",
            GoalId::Eat => "eat",
            GoalId::Drink => "drink",
            GoalId::Relieve => "relieve",
        }
    }

    /// The emoji shown above a unit's head while this goal is in charge —
    /// see [`crate::sim::GameEntity::current_goal`]. Single codepoints only,
    /// deliberately: a multi-codepoint sequence (a skin tone, a ZWJ join)
    /// risks falling back to its unjoined parts on a renderer that does not
    /// merge them, which is a worse failure than a plainer emoji.
    pub const fn emoji(self) -> &'static str {
        match self {
            GoalId::Idle => "💤",
            GoalId::Wander => "🚶",
            GoalId::Eat => "🍎",
            GoalId::Drink => "💧",
            GoalId::Relieve => "🚽",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Ticks a goal that reported itself impossible is held off the top of the
/// list. The old `Walker::REPLAN_DELAY`, generalised, and the same reasoning:
/// finding out that something cannot be done is the expensive miss, and paying
/// for it every tick is a frame-rate cliff that only appears on maps with a
/// sealed room in them. Roughly half a second at the fixed timestep.
pub const BLOCKED_TICKS: u16 = 32;

/// How many times [`BLOCKED_TICKS`] may double for a goal that keeps being
/// impossible: 32 ticks, then 64, up to about half a minute.
///
/// The doubling is what stops two goals from taking turns forever. A hungry
/// human on a map with no fridge would otherwise stop wandering every half
/// second to look for one, dropping its route each time — a visible stutter,
/// and a far route re-planned per human twice a second. The map a simulation
/// runs on does not change, so an impossible goal stays impossible; the cap is
/// only there so a crowd that clears from a doorway is noticed eventually.
pub const MAX_BACKOFF: u8 = 6;

/// The priority list.
///
/// **Arranged from scratch every tick.** The brain zeroes every priority, then
/// every routine raises what it cares about; so a priority is always what the
/// routines want *now*, and a routine that stops caring about a goal needs to
/// do nothing to let it fall.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Goals {
    priority: [f32; GoalId::COUNT],
    /// Ticks before a goal that reported itself impossible may top the list
    /// again. Deliberately *not* the priority: a routine will keep raising a
    /// goal it cannot see is impossible, and zeroing the priority would make
    /// the brains menu lie about what the routine wants.
    cooldown: [u16; GoalId::COUNT],
    /// Consecutive times a goal has been found impossible, which is how long
    /// its next cooldown is. Reset by any sign of progress.
    strikes: [u8; GoalId::COUNT],
    top: GoalId,
}

impl Default for Goals {
    fn default() -> Goals {
        Goals::new()
    }
}

impl Goals {
    pub const fn new() -> Goals {
        Goals {
            priority: [0.0; GoalId::COUNT],
            cooldown: [0; GoalId::COUNT],
            strikes: [0; GoalId::COUNT],
            top: GoalId::Idle,
        }
    }

    pub fn priority(&self, goal: GoalId) -> f32 {
        self.priority[goal.index()]
    }

    pub fn cooldown(&self, goal: GoalId) -> u16 {
        self.cooldown[goal.index()]
    }

    /// The goal in charge, as of the last [`Goals::settle`].
    pub fn top(&self) -> GoalId {
        self.top
    }

    /// Raise `goal` to at least `at_least`.
    ///
    /// A max, not an assignment, so two routines pushing one goal up cannot
    /// undo each other and the order they run in does not matter to the
    /// result.
    pub fn raise_to(&mut self, goal: GoalId, at_least: f32) {
        let priority = &mut self.priority[goal.index()];
        // `f32::max` ignores a NaN argument, which is the right answer here:
        // a routine that divided by zero has not asked for anything.
        *priority = priority.max(at_least);
    }

    /// Zero every priority, ready for the routines to arrange the list again.
    pub(super) fn clear_priorities(&mut self) {
        self.priority = [0.0; GoalId::COUNT];
    }

    /// One tick off every cooldown.
    pub(super) fn tick_cooldowns(&mut self) {
        for cooldown in &mut self.cooldown {
            *cooldown = cooldown.saturating_sub(1);
        }
    }

    /// `goal` found out it cannot be done from here: hold it off the top for
    /// [`BLOCKED_TICKS`], doubled for every time in a row this has happened.
    pub(super) fn block(&mut self, goal: GoalId) {
        if goal == GoalId::Idle {
            return;
        }
        let i = goal.index();
        let shift = self.strikes[i].min(MAX_BACKOFF);
        self.cooldown[i] = BLOCKED_TICKS << shift;
        self.strikes[i] = self.strikes[i].saturating_add(1);
    }

    /// `goal` got somewhere: its next failure starts the back-off again from
    /// the bottom.
    pub(super) fn progressed(&mut self, goal: GoalId) {
        self.strikes[goal.index()] = 0;
    }

    /// Which goal *should* be in charge: the highest priority not on a
    /// cooldown.
    ///
    /// **An argmax with a strict `>`, not a sort.** Ties go to the lower
    /// [`GoalId`], so the same priorities always settle the same way; a NaN
    /// is never greater than anything and so never wins, which turns a routine
    /// that divides by zero into "not chosen" rather than into a winner that
    /// depends on where the NaN sat; and nothing allocates. [`GoalId::Idle`]
    /// is where the search starts, so it is what is left when nothing is
    /// wanted at all.
    pub fn best(&self) -> GoalId {
        let mut best = GoalId::Idle;
        let mut value = self.priority(GoalId::Idle);
        if value.is_nan() {
            value = f32::NEG_INFINITY;
        }
        for goal in GoalId::ALL {
            if goal == GoalId::Idle || self.cooldown(goal) > 0 {
                continue;
            }
            let priority = self.priority(goal);
            if priority > value {
                best = goal;
                value = priority;
            }
        }
        best
    }

    /// Put the best goal in charge. Returns `(old, new)` only when that changed
    /// who is in charge — exactly the moment a handover is owed.
    pub fn settle(&mut self) -> Option<(GoalId, GoalId)> {
        let best = self.best();
        if best == self.top {
            return None;
        }
        let old = std::mem::replace(&mut self.top, best);
        Some((old, best))
    }

    /// Every goal with its priority, highest first, ties in goal order.
    ///
    /// The sorting, allocating version of [`Goals::best`], and only for the
    /// brains menu — nothing in a tick calls it.
    pub fn ranked(&self) -> Vec<(GoalId, f32)> {
        let mut ranked: Vec<(GoalId, f32)> =
            GoalId::ALL.iter().map(|&goal| (goal, self.priority(goal))).collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked
    }
}

/// Everything an executor may look at while it works, and the two things it
/// may change: its unit's memory, and the task queue.
///
/// **The body and hands are read-only here.** A goal decides; the tasks it queues
/// act — food goes into a hand when a [`TakeItem`](super::tasks::TakeItem)
/// finishes, not when a goal hears that it did. Which also means a goal that
/// misses hearing about a task, because something more important took over,
/// has lost nothing: what the task did is done.
///
/// No walker either. An executor decides where to go by queueing a task, and a
/// route is asked for once per walk started — which is what keeps the number
/// of searches a function of how many decisions were made rather than of how
/// many ticks went by.
pub struct GoalCtx<'a> {
    pub think: &'a Think<'a>,
    pub body: &'a Body,
    pub perception: &'a Perception,
    /// Stats, and which processes are running. `None` for a kind that has no
    /// needs.
    pub biology: Option<&'a Biology>,
    pub memory: &'a mut Memory,
    pub tasks: &'a mut Tasks,
    /// What it is carrying, **read-only**: its hand, and what it has stowed.
    /// `None` for a kind that carries nothing.
    pub inventory: Option<&'a Inventory>,
    /// Who refused the move that ended the last task, when a body did.
    ///
    /// A failed walk is two different things — somebody is in the way, and
    /// bodies move; or there is no way there, and walls do not — and a goal
    /// cannot tell which from [`TaskResult::Failed`] alone.
    pub blocked_by: Option<Uid>,
    /// The task whose ending `last` is about, when one ended since the
    /// executor was last asked. A goal with two timed steps cannot tell a
    /// finished wait from a finished meal by the result alone.
    pub finished: Option<Task>,
}

/// What an executor says about its goal after being asked to work on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GoalProgress {
    /// Still working, or this tick's attempt missed cheaply and is worth
    /// retrying immediately — a wanderer's unlucky dice roll is that.
    Working,
    /// Done. The brain clears whatever is still queued. **It does not hand
    /// over** — who is in charge is the routines' to say: if the routine that
    /// raised this goal still wants it, it stays on top and is asked again next
    /// tick, which starts it over. That is a second meal for someone one meal
    /// did not fill.
    Achieved,
    /// Cannot be done from here, and finding that out was expensive. Held off
    /// for [`BLOCKED_TICKS`] (and longer, if it keeps happening).
    Blocked,
}

/// How one goal is pursued.
///
/// A `Box<dyn GoalExecutor>` the brain owns for the entity's whole life,
/// constructed at spawn and never dropped or rebuilt. **Its fields are the
/// saved state** — no blob, no `Box<dyn Any>`, no serialization. What
/// [`GoalExecutor::deprioritized`] is for is the *queue*, which the executor
/// does not own: the brain clears it on every handover, because tasks belong
/// to whoever is in charge. So `deprioritized` notes the stage and drops what
/// will be stale, and [`GoalExecutor::prioritized`] re-queues the task its
/// remembered stage implies — on the front, because a resume is a
/// prerequisite.
///
/// The handover order is fixed: **old executor told → current task abandoned
/// and walk halted → queue cleared → new executor told.** So `deprioritized`
/// can still see its own tasks and `prioritized` always sees an empty queue.
pub trait GoalExecutor: Send + Sync {
    fn goal(&self) -> GoalId;

    /// Just put in charge. The queue is empty and nothing has ended that this
    /// goal queued.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        let _ = ctx;
    }

    /// About to stop being in charge. The queue still holds its tasks, and
    /// `ctx.tasks.result()` / `ctx.finished` are the last task's ending if it
    /// had not been heard yet — this is the last chance to hear it.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        let _ = ctx;
    }

    /// Work on the goal. Called only when the goal has just been put in charge
    /// or when no task is current — which is the tick after one ended, so
    /// `last` is what that task wrote, or [`TaskResult::InProgress`] when
    /// nothing has ended since the last call. `ctx.finished` says which task
    /// it was.
    ///
    /// **Must either queue a task, or return something other than
    /// [`GoalProgress::Working`], unless the miss was cheap.** An executor
    /// that returns `Working` with nothing queued is asked again next tick,
    /// and one that pays for a search to get there is the per-agent,
    /// per-tick planning that killed the earlier prototype.
    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress;

    /// What this executor would tell a debugger. Allocates, and is only asked
    /// about the one unit somebody has selected.
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_wanted_means_idle() {
        let mut goals = Goals::new();
        assert_eq!(goals.best(), GoalId::Idle);
        assert_eq!(goals.settle(), None, "idle is where a brain starts");
    }

    #[test]
    fn the_most_wanted_goal_is_the_one_in_charge() {
        let mut goals = Goals::new();
        goals.raise_to(GoalId::Wander, 0.1);
        goals.raise_to(GoalId::Eat, 1.4);
        assert_eq!(goals.settle(), Some((GoalId::Idle, GoalId::Eat)));
        assert_eq!(goals.top(), GoalId::Eat);
        assert_eq!(goals.settle(), None, "no change, no handover");
    }

    #[test]
    fn two_goals_at_the_same_priority_are_settled_by_goal_order() {
        let mut goals = Goals::new();
        goals.raise_to(GoalId::Eat, 0.5);
        goals.raise_to(GoalId::Wander, 0.5);
        assert_eq!(goals.best(), GoalId::Wander);
    }

    #[test]
    fn a_priority_that_is_not_a_number_never_reaches_the_top() {
        let mut goals = Goals::new();
        goals.priority[GoalId::Eat.index()] = f32::NAN;
        goals.raise_to(GoalId::Wander, 0.1);
        assert_eq!(goals.best(), GoalId::Wander);

        // Not even over nothing at all.
        let mut goals = Goals::new();
        goals.priority[GoalId::Eat.index()] = f32::NAN;
        assert_eq!(goals.best(), GoalId::Idle);
    }

    #[test]
    fn raising_a_goal_never_lowers_it() {
        let mut goals = Goals::new();
        goals.raise_to(GoalId::Eat, 0.8);
        goals.raise_to(GoalId::Eat, 0.3);
        goals.raise_to(GoalId::Eat, f32::NAN);
        assert_eq!(goals.priority(GoalId::Eat), 0.8);
    }

    #[test]
    fn a_goal_on_a_cooldown_cannot_reach_the_top_however_badly_it_is_wanted() {
        let mut goals = Goals::new();
        goals.raise_to(GoalId::Wander, 0.1);
        goals.raise_to(GoalId::Eat, 100.0);
        goals.block(GoalId::Eat);
        assert_eq!(goals.best(), GoalId::Wander);

        for _ in 0..BLOCKED_TICKS {
            assert_eq!(goals.best(), GoalId::Wander);
            goals.tick_cooldowns();
        }
        assert_eq!(goals.best(), GoalId::Eat, "the cooldown ran out");
    }

    #[test]
    fn a_cooldown_does_not_touch_the_priority_the_routine_asked_for() {
        // The brains menu shows what the routines want, and a goal held off
        // is still wanted.
        let mut goals = Goals::new();
        goals.raise_to(GoalId::Eat, 1.4);
        goals.block(GoalId::Eat);
        assert_eq!(goals.priority(GoalId::Eat), 1.4);
    }

    #[test]
    fn a_goal_that_keeps_being_impossible_is_held_off_for_longer_each_time() {
        let mut goals = Goals::new();
        let mut lengths = Vec::new();
        for _ in 0..10 {
            goals.block(GoalId::Eat);
            lengths.push(goals.cooldown(GoalId::Eat));
        }
        assert_eq!(&lengths[..3], [BLOCKED_TICKS, BLOCKED_TICKS * 2, BLOCKED_TICKS * 4]);
        assert_eq!(*lengths.last().unwrap(), BLOCKED_TICKS << MAX_BACKOFF);

        // ...and one sign of progress starts it from the bottom again.
        goals.progressed(GoalId::Eat);
        goals.block(GoalId::Eat);
        assert_eq!(goals.cooldown(GoalId::Eat), BLOCKED_TICKS);
    }

    #[test]
    fn idle_cannot_be_blocked() {
        // It is what is left when nothing else can be done, so holding it off
        // would leave nothing.
        let mut goals = Goals::new();
        goals.block(GoalId::Idle);
        assert_eq!(goals.cooldown(GoalId::Idle), 0);
    }

    #[test]
    fn every_goal_has_an_emoji() {
        for goal in GoalId::ALL {
            assert!(!goal.emoji().is_empty(), "{goal:?} has no emoji");
        }
    }

    #[test]
    fn the_ranked_list_is_highest_first_with_ties_in_goal_order() {
        let mut goals = Goals::new();
        goals.raise_to(GoalId::Relieve, 0.1);
        goals.raise_to(GoalId::Drink, 0.1);
        goals.raise_to(GoalId::Eat, 0.1);
        goals.raise_to(GoalId::Wander, 0.1);
        assert_eq!(
            goals.ranked(),
            [
                (GoalId::Wander, 0.1),
                (GoalId::Eat, 0.1),
                (GoalId::Drink, 0.1),
                (GoalId::Relieve, 0.1),
                (GoalId::Idle, 0.0)
            ]
        );
    }
}
