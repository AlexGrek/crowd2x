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

use crate::save::{invalid, pb, Invalid};
use crate::sim::biology::Biology;
use crate::sim::catalogue::catalogue;
use crate::sim::entity::{Body, Think};
use crate::sim::inventory::Inventory;
use crate::sim::job::Job;
use crate::sim::uid::Uid;

use super::goals;
use super::memory::Memory;
use super::perception::Perception;
use super::task::{Task, TaskResult, Tasks};

/// Everything about one goal: its row in [`GoalId`]'s catalogue.
pub struct GoalDef {
    /// How it reads on the brains menu.
    pub name: &'static str,
    /// What is shown above a unit's head while it is in charge. See
    /// [`GoalId::emoji`].
    pub emoji: &'static str,
    /// A fresh executor for it, for every brain a person is built with
    /// ([`Brain::human`](super::Brain::human)); `None` for a goal nobody
    /// pursues.
    pub executor: Option<fn() -> Box<dyn GoalExecutor>>,
}

catalogue! {
    /// Every goal a brain can hold.
    ///
    /// An enum over a fixed array rather than a map of strings: a typo in a
    /// variant is a compile error where a typo in a string is a runtime one,
    /// the numbers are the storage order (as `map::ObjectLayer`'s are) and what
    /// a save keeps, and iterating in the order listed is what the determinism
    /// tests depend on. **Ties go to the goal listed first** ([`Goals::best`]),
    /// which is why the jobs' goals come after every need.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
    pub enum GoalId: GoalDef {
        /// Nothing to do. The goal in charge when no other goal is wanted at
        /// all, which is why it has no executor anywhere and cannot be put on a
        /// cooldown.
        Idle = 0 => GoalDef { name: "idle", emoji: "💤", executor: None },
        Wander = 1 => GoalDef { name: "wander", emoji: "🚶", executor: Some(|| Box::new(goals::WanderGoal::new())) },
        Eat = 2 => GoalDef { name: "eat", emoji: "🍎", executor: Some(|| Box::new(goals::EatGoal::new())) },
        Drink = 3 => GoalDef { name: "drink", emoji: "💧", executor: Some(|| Box::new(goals::DrinkGoal::new())) },
        Relieve = 4 => GoalDef { name: "relieve", emoji: "🚽", executor: Some(|| Box::new(goals::RelieveGoal::new())) },
        Play = 5 => GoalDef { name: "play", emoji: "🎮", executor: Some(|| Box::new(goals::PlayGoal::new())) },
        Sleep = 6 => GoalDef { name: "sleep", emoji: "😴", executor: Some(|| Box::new(goals::SleepGoal::new())) },
        /// Carry out an order to salvage a heap of scrap
        /// ([`SalvageGoal`](super::goals::SalvageGoal)).
        Salvage = 7 => GoalDef { name: "salvage", emoji: "🔧", executor: Some(|| Box::new(goals::SalvageGoal::new())) },
        /// Go to one's own desk and work a shift at it
        /// ([`WorkGoal`](super::goals::WorkGoal)). After every need, so every
        /// need it ties with wins the tie.
        Work = 8 => GoalDef { name: "work", emoji: "💼", executor: Some(|| Box::new(goals::WorkGoal::new())) },
        /// Bring scrap in from the wasteland and store it in one's own bin
        /// ([`CollectGoal`](super::goals::CollectGoal)): a scrap collector's
        /// shift. After work, for the reason work is after every need.
        Collect = 9 => GoalDef { name: "collect", emoji: "🧲", executor: Some(|| Box::new(goals::CollectGoal::new())) },
        /// Carry the dead to one's own crematorium
        /// ([`BuryGoal`](super::goals::BuryGoal)): an undertaker's watch.
        Bury = 10 => GoalDef { name: "bury", emoji: "⚰", executor: Some(|| Box::new(goals::BuryGoal::new())) },
        /// Smelt scrap into steel at one's own furnace
        /// ([`SmeltGoal`](super::goals::SmeltGoal)): a steelworker's shift.
        Smelt = 11 => GoalDef { name: "smelt", emoji: "🔥", executor: Some(|| Box::new(goals::SmeltGoal::new())) },
        /// Carry scrap to the steel factory and steel to the warehouse
        /// ([`HaulGoal`](super::goals::HaulGoal)): a courier's shift.
        Haul = 12 => GoalDef { name: "haul", emoji: "📦", executor: Some(|| Box::new(goals::HaulGoal::new())) },
        /// Sow, tend and harvest one's own field, and store the wheat at one's
        /// own grain bin ([`FarmGoal`](super::goals::FarmGoal)): a farmer's
        /// shift.
        Farm = 13 => GoalDef { name: "farm", emoji: "🌱", executor: Some(|| Box::new(goals::FarmGoal::new())) },
        Repair = 14 => GoalDef { name: "repair", emoji: "🔧", executor: Some(|| Box::new(goals::RepairGoal::new())) },
        Cook = 15 => GoalDef { name: "cook", emoji: "??", executor: Some(|| Box::new(goals::CookGoal::new())) },
        Recover = 16 => GoalDef { name: "recover", emoji: "\u{1f3e5}", executor: Some(|| Box::new(goals::RecoverGoal::new())) },
        Care = 17 => GoalDef { name: "care", emoji: "\u{1fa7a}", executor: Some(|| Box::new(goals::CareGoal::new())) },
        /// Run for the nearest way in, and shelter inside until the fright is
        /// over ([`FleeGoal`](super::goals::FleeGoal)): over everything else
        /// while a deathdog, or somebody running from one, scared it lately.
        Flee = 18 => GoalDef { name: "flee", emoji: "😱", executor: Some(|| Box::new(goals::FleeGoal::new())) },
        /// A deathdog's: run down somebody outside the walls and bite
        /// ([`HuntGoal`](super::goals::HuntGoal)). No executor here, so no
        /// person carries one: `Brain::deathdog` builds it.
        Hunt = 19 => GoalDef { name: "hunt", emoji: "🐺", executor: None },
        /// A deathdog's: circle the walls, waiting
        /// ([`ProwlGoal`](super::goals::ProwlGoal)).
        Prowl = 20 => GoalDef { name: "prowl", emoji: "🐾", executor: None },
        /// A deathdog's: by day, walk off into the wasteland
        /// ([`DepartGoal`](super::goals::DepartGoal)).
        Depart = 21 => GoalDef { name: "depart", emoji: "🌅", executor: None },
        Fish = 22 => GoalDef { name: "fish", emoji: "🎣", executor: Some(|| Box::new(goals::FishGoal::new())) },
        /// Go over to somebody seen while wandering and have a chat
        /// ([`ChatGoal`](super::goals::ChatGoal)), once both have agreed to
        /// one: over wandering, under everything else.
        Chat = 23 => GoalDef { name: "chat", emoji: "💬", executor: Some(|| Box::new(goals::ChatGoal::new())) },
        Decorate = 24 => GoalDef { name: "decorate", emoji: "🌷", executor: Some(|| Box::new(goals::DecorateGoal::new())) },
        /// Look over the district for dirt and mop it up, or walk round
        /// with none to find ([`CleanGoal`](super::goals::CleanGoal)): a
        /// cleaner's shift.
        Clean = 25 => GoalDef { name: "clean", emoji: "🧹", executor: Some(|| Box::new(goals::CleanGoal::new())) },
        /// Feed one's pet dog: dog food out of a fridge, given to the dog
        /// ([`DogCareGoal`](super::goals::DogCareGoal)). Over wandering and
        /// under a night in bed and every need committed to.
        DogCare = 26 => GoalDef { name: "dog care", emoji: "🦴", executor: Some(|| Box::new(goals::DogCareGoal::new())) },
        /// A pet dog's: stay with its owner, roaming freely within range
        /// ([`FollowGoal`](super::goals::FollowGoal)). No executor here, so
        /// no person carries one: `Brain::dog` builds it.
        Follow = 27 => GoalDef { name: "follow", emoji: "🐕", executor: None },
        /// A pet dog's: empty its bowel out of doors
        /// ([`DefecateGoal`](super::goals::DefecateGoal)).
        Defecate = 28 => GoalDef { name: "defecate", emoji: "💩", executor: None },
        /// Fill the pocket with a meal and a drink for a trip out
        /// ([`ProvisionGoal`](super::goals::ProvisionGoal)): part of getting
        /// ready, for the jobs worked out in the wasteland.
        Provision = 29 => GoalDef { name: "provision", emoji: "🎒", executor: Some(|| Box::new(goals::ProvisionGoal::new())) },
    }
}

impl GoalId {
    pub const fn name(self) -> &'static str {
        self.def().name
    }

    /// The emoji shown above a unit's head while this goal is in charge —
    /// see [`crate::sim::GameEntity::current_goal`]. Single codepoints only,
    /// deliberately: a multi-codepoint sequence (a skin tone, a ZWJ join)
    /// risks falling back to its unjoined parts on a renderer that does not
    /// merge them, which is a worse failure than a plainer emoji.
    pub const fn emoji(self) -> &'static str {
        self.def().emoji
    }

    const fn index(self) -> usize {
        self as usize
    }

    /// Whether, in charge, it **holds** against a rival wanted only a little
    /// more than it is ([`HOLD`]): anything a unit is actually doing. Not the
    /// floors a list rests on — idling, wandering about, a dog at heel, a
    /// deathdog prowling the walls — which are what a unit does with nothing
    /// to do, and give way to anything at all.
    pub const fn holds(self) -> bool {
        !matches!(self, GoalId::Idle | GoalId::Wander | GoalId::Follow | GoalId::Prowl)
    }

    /// The goal a save numbered `number`, or why not.
    pub(crate) fn restore(number: i32, what: &str) -> Result<GoalId, Invalid> {
        GoalId::from_number(number).ok_or_else(|| Invalid::new(format!("{what}: no goal numbered {number}")))
    }
}

/// Ticks a goal that reported itself impossible is held off the top of the
/// list. The old `Walker::REPLAN_DELAY`, generalised, and the same reasoning:
/// finding out that something cannot be done is the expensive miss, and paying
/// for it every tick is a frame-rate cliff that only appears on maps with a
/// sealed room in them. Roughly half a second at the fixed timestep.
pub const BLOCKED_TICKS: u16 = 32;

/// How much more a rival has to be wanted than the goal in charge to take
/// over from it ([`Goals::settle`]): **what a unit is doing counts.** Fifteen
/// points of a need on the routines' scale (a need's priority is its level
/// over 50), so a meal under way is not dropped for a drink wanted a little
/// more, a night in bed not for a dog that wants feeding, an errand not for
/// bedtime; and a shift ([`WORKING`], 1.5) is left only for a need past 90 —
/// starving, parched, about to wet itself — or for danger
/// ([`FLEEING`], 3.0, over everything however it is held).
///
/// Only while the goal in charge still holds ([`GoalId::holds`]), is still
/// wanted at all and is not held off: a goal its routine let go of, or one
/// that found itself impossible, holds nothing. A routine that has to break
/// in on something on purpose — getting ready for a trip out, before the
/// trip itself — raises its goal past the hold
/// ([`GETTING_READY`](super::routines::GETTING_READY)).
///
/// [`WORKING`]: super::routines::WORKING
/// [`FLEEING`]: super::routines::FLEEING
pub const HOLD: f32 = 0.3;

// `Goals::breaking_in` is a bit per goal.
const _: () = assert!(GoalId::COUNT <= u64::BITS as usize);

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
///
/// **Beside every priority, an urgency**: how soon, where the priority is how
/// badly. Arranged the same way, by the same routines, and read for one thing
/// — how much of a hurry the unit is in while the goal on top is in charge
/// ([`Walker::hurry`](crate::sim::walker::Walker::hurry)). Two numbers, not
/// one: getting ready for a trip out is wanted over a shift and is no hurry,
/// and a shift is a hurry only for somebody late for it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Goals {
    priority: [f32; GoalId::COUNT],
    /// How urgent each goal is, 0 (no hurry) to 1 (flat out), raised by the
    /// routines with the priorities ([`Goals::hurry`]).
    urgency: [f32; GoalId::COUNT],
    /// Ticks before a goal that reported itself impossible may top the list
    /// again. Deliberately *not* the priority: a routine will keep raising a
    /// goal it cannot see is impossible, and zeroing the priority would make
    /// the brains menu lie about what the routine wants.
    cooldown: [u16; GoalId::COUNT],
    /// Consecutive times a goal has been found impossible, which is how long
    /// its next cooldown is. Reset by any sign of progress.
    strikes: [u8; GoalId::COUNT],
    /// A bit per goal raised this tick to break in on whatever is in charge
    /// ([`Goals::break_in`]): the goal in charge does not hold against it.
    /// Arranged with the priorities, every tick.
    breaking_in: u64,
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
            urgency: [0.0; GoalId::COUNT],
            cooldown: [0; GoalId::COUNT],
            strikes: [0; GoalId::COUNT],
            breaking_in: 0,
            top: GoalId::Idle,
        }
    }

    pub fn priority(&self, goal: GoalId) -> f32 {
        self.priority[goal.index()]
    }

    /// How urgent `goal` is, 0 to 1: how much of a hurry a unit is in while
    /// it is in charge.
    pub fn urgency(&self, goal: GoalId) -> f32 {
        self.urgency[goal.index()]
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

    /// Raise `goal` to at least `at_least`, as [`Goals::raise_to`] does, **and
    /// let it break in** on whatever is in charge this tick: the goal in charge
    /// does not hold against it ([`HOLD`]), so it takes over the moment it is
    /// wanted more at all. For a routine whose whole point is to stop
    /// something going on — getting ready before a trip out, which stops the
    /// shift at the bin — and never for an ordinary want.
    pub fn break_in(&mut self, goal: GoalId, at_least: f32) {
        self.raise_to(goal, at_least);
        self.breaking_in |= 1 << goal.index();
    }

    /// Whether `goal` is breaking in this tick ([`Goals::break_in`]).
    pub fn is_breaking_in(&self, goal: GoalId) -> bool {
        self.breaking_in & (1 << goal.index()) != 0
    }

    /// Make `goal` at least `urgency` urgent (0 to 1), whoever wants it.
    ///
    /// A max, like [`Goals::raise_to`], and for the same reason. Urgency is
    /// the goal's and not the routine's: a meal wanted by the routine getting
    /// a worker ready is as urgent as the hunger behind it says.
    pub fn hurry(&mut self, goal: GoalId, urgency: f32) {
        // Not `max(urgency.min(1.0))` alone: `NaN.min(1.0)` is 1, and a
        // routine that divided by zero has not asked for a run.
        if urgency > 0.0 {
            let current = &mut self.urgency[goal.index()];
            *current = current.max(urgency.min(1.0));
        }
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Goals {
        let Goals { priority, urgency, cooldown, strikes, breaking_in, top } = self;
        pb::Goals {
            top: top.number(),
            slots: GoalId::ALL
                .iter()
                .map(|&goal| pb::GoalSlot {
                    goal: goal.number(),
                    priority: priority[goal.index()],
                    cooldown: cooldown[goal.index()] as u32,
                    strikes: strikes[goal.index()] as u32,
                    urgency: urgency[goal.index()],
                    breaking_in: breaking_in & (1 << goal.index()) != 0,
                })
                .collect(),
        }
    }

    /// The list as saved. A goal the save does not mention — one added since
    /// — starts where it does in a new brain: unwanted, not held off.
    pub(crate) fn restore(saved: &pb::Goals) -> Result<Goals, Invalid> {
        let mut goals = Goals::new();
        goals.top = GoalId::restore(saved.top, "top goal")?;
        for slot in &saved.slots {
            let goal = GoalId::restore(slot.goal, "goal")?;
            let (Ok(cooldown), Ok(strikes)) = (u16::try_from(slot.cooldown), u8::try_from(slot.strikes)) else {
                return invalid(format!("{} held off {} ticks after {} strikes", goal.name(), slot.cooldown, slot.strikes));
            };
            if !(0.0..=1.0).contains(&slot.urgency) {
                return invalid(format!("{} is {} urgent", goal.name(), slot.urgency));
            }
            goals.priority[goal.index()] = slot.priority;
            goals.urgency[goal.index()] = slot.urgency;

            goals.cooldown[goal.index()] = cooldown;
            goals.strikes[goal.index()] = strikes;
            if slot.breaking_in {
                goals.breaking_in |= 1 << goal.index();
            }
        }
        Ok(goals)
    }

    pub(crate) fn suppress(&mut self, goal: GoalId) {
        self.priority[goal.index()] = 0.0;
        self.urgency[goal.index()] = 0.0;
        self.breaking_in &= !(1 << goal.index());
    }

    /// Zero every priority and urgency, ready for the routines to arrange
    /// the list again.
    pub(super) fn clear_priorities(&mut self) {
        self.priority = [0.0; GoalId::COUNT];
        self.urgency = [0.0; GoalId::COUNT];
        self.breaking_in = 0;
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

    /// Put the best goal in charge — unless the goal in charge holds against
    /// it ([`HOLD`]). Returns `(old, new)` only when that changed who is in
    /// charge — exactly the moment a handover is owed.
    pub fn settle(&mut self) -> Option<(GoalId, GoalId)> {
        let best = self.best();
        if best == self.top || self.holds_against(best) {
            return None;
        }
        let old = std::mem::replace(&mut self.top, best);
        Some((old, best))
    }

    /// Whether the goal in charge keeps charge against `rival`, the best of the
    /// rest: it holds ([`GoalId::holds`]), is still wanted, is not held off,
    /// and `rival` is wanted by no more than [`HOLD`] over it. A NaN is never
    /// wanted, so it never holds; and a rival is compared with a plain `>`,
    /// so a NaN never takes over.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn holds_against(&self, rival: GoalId) -> bool {
        let top = self.top;
        let mine = self.priority(top);
        top.holds()
            && self.cooldown(top) == 0
            && mine > 0.0
            && !self.is_breaking_in(rival)
            && !(self.priority(rival) > mine + HOLD)
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
    /// What it does for a living, if anything: where its desk is, and its
    /// hours. Read-only — a job is handed out and taken back from outside the
    /// world (`GameState::give_a_job`), never by a goal.
    pub job: Option<&'a Job>,
    /// Whether it has got ready for a trip out for its job and is setting
    /// out ([`ReadyRoutine`](super::routines::ReadyRoutine)) — read-only, the
    /// routine's to say. What `CollectGoal` asks before it leaves for a heap:
    /// one not ready goes to its bin first, where getting ready starts.
    pub ready_to_go_out: bool,
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
/// saved state** — no blob, no `Box<dyn Any>` — and they are what a save file
/// keeps of it too ([`GoalExecutor::save`]). What
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

    /// Asked every tick it is in charge while a task of its, `current`, is
    /// going on: what will not wait for the task to end. `true` cuts the task
    /// short — told so, as a hand-over tells it — and has [`process`] asked
    /// straight away, with nothing ended. Nothing to keep up with, for every
    /// goal but a cleaner's, whose look over the district is every 27 minutes
    /// whatever it is doing ([`CleanGoal`](super::goals::CleanGoal)).
    ///
    /// Asked of the whole crowd every tick: anything but a counter compared
    /// is for the tick it is due.
    ///
    /// [`process`]: GoalExecutor::process
    fn keep_up(&mut self, ctx: &mut GoalCtx<'_>, current: &Task) -> bool {
        let _ = (ctx, current);
        false
    }

    /// What this executor would tell a debugger. Allocates, and is only asked
    /// about the one unit somebody has selected.
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// Its fields, for a save (`crate::save`). No default, on purpose: an
    /// executor whose state a save forgot would come back from a load at
    /// some other stage of its goal than the one it was at.
    fn save(&self) -> pb::Executor;

    /// Put back what [`GoalExecutor::save`] kept, into an executor built
    /// fresh with its brain. A state of another executor's shape is refused.
    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid>;
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

    /// `goals` with `top` put in charge at `wanted`, then the list arranged
    /// afresh with `top` at `still` — the next tick.
    fn in_charge(top: GoalId, wanted: f32, still: f32) -> Goals {
        let mut goals = Goals::new();
        goals.raise_to(top, wanted);
        goals.settle();
        assert_eq!(goals.top(), top);
        goals.clear_priorities();
        goals.raise_to(top, still);
        goals
    }

    /// What a unit is doing counts: a rival takes over only when it is
    /// wanted by more than the hold over the goal in charge, and a tie with
    /// the hold is not enough.
    #[test]
    fn the_goal_in_charge_holds_against_a_rival_wanted_only_a_little_more() {
        let mut goals = in_charge(GoalId::Eat, 1.2, 1.2);
        goals.raise_to(GoalId::Drink, 1.2 + HOLD * 0.9);
        assert_eq!(goals.best(), GoalId::Drink, "wanted more, on a fresh choice");
        assert_eq!(goals.settle(), None, "the meal goes on");
        assert_eq!(goals.top(), GoalId::Eat);

        let mut goals = in_charge(GoalId::Eat, 1.2, 1.2);
        goals.raise_to(GoalId::Drink, 1.2 + HOLD);
        assert_eq!(goals.settle(), None, "a tie with the hold holds");

        let mut goals = in_charge(GoalId::Eat, 1.2, 1.2);
        goals.raise_to(GoalId::Drink, 1.2 + HOLD + 0.01);
        assert_eq!(goals.settle(), Some((GoalId::Eat, GoalId::Drink)));
    }

    /// Wandering, idling, a dog at heel and a deathdog's prowl are what a
    /// unit does with nothing to do: they give way to anything wanted more.
    #[test]
    fn nothing_much_holds_nothing() {
        for floor in [GoalId::Wander, GoalId::Follow, GoalId::Prowl] {
            assert!(!floor.holds(), "{floor:?}");
            let mut goals = in_charge(floor, 0.1, 0.1);
            goals.raise_to(GoalId::Play, 0.12);
            assert_eq!(goals.settle(), Some((floor, GoalId::Play)), "{floor:?}");
        }
        assert!(GoalId::Eat.holds() && GoalId::Work.holds() && GoalId::Chat.holds() && GoalId::Play.holds());
    }

    /// A goal its routine let go of, one that found itself impossible, and
    /// one whose priority is no number hold nothing.
    #[test]
    fn a_goal_let_go_of_or_held_off_holds_nothing() {
        let mut goals = in_charge(GoalId::Eat, 1.2, 0.0);
        goals.raise_to(GoalId::Wander, 0.1);
        assert_eq!(goals.settle(), Some((GoalId::Eat, GoalId::Wander)), "let go of");

        let mut goals = in_charge(GoalId::Eat, 1.2, 1.2);
        goals.block(GoalId::Eat);
        goals.raise_to(GoalId::Wander, 0.1);
        assert_eq!(goals.settle(), Some((GoalId::Eat, GoalId::Wander)), "held off");

        let mut goals = in_charge(GoalId::Eat, 1.2, 1.2);
        goals.priority[GoalId::Eat.index()] = f32::NAN;
        goals.raise_to(GoalId::Wander, 0.1);
        assert_eq!(goals.settle(), Some((GoalId::Eat, GoalId::Wander)), "not a number");
    }

    /// A goal raised to break in takes over the moment it is wanted more at
    /// all, hold or no hold — and only on the tick it was raised so.
    #[test]
    fn a_goal_raised_to_break_in_is_not_held_against() {
        let mut goals = in_charge(GoalId::Collect, 1.5, 1.5);
        goals.break_in(GoalId::Drink, 1.6);
        assert!(goals.is_breaking_in(GoalId::Drink));
        assert_eq!(goals.settle(), Some((GoalId::Collect, GoalId::Drink)));

        let mut goals = in_charge(GoalId::Collect, 1.5, 1.5);
        goals.break_in(GoalId::Drink, 1.4);
        assert_eq!(goals.settle(), None, "breaking in is not wanting more");

        goals.clear_priorities();
        assert!(!goals.is_breaking_in(GoalId::Drink), "arranged afresh every tick");
        goals.break_in(GoalId::Drink, 1.6);
        goals.suppress(GoalId::Drink);
        assert!(!goals.is_breaking_in(GoalId::Drink), "suppressed, it breaks in on nothing");
    }

    /// The goal in charge and what is breaking in come back from a save, so
    /// a loaded brain holds — and gives way — exactly as the saved one would.
    #[test]
    fn the_hold_and_what_breaks_in_come_back_from_a_save() {
        let mut goals = in_charge(GoalId::Collect, 1.5, 1.5);
        goals.break_in(GoalId::Drink, 1.6);
        let restored = Goals::restore(&goals.save()).unwrap();
        assert_eq!(restored, goals);
        assert!(restored.is_breaking_in(GoalId::Drink) && !restored.is_breaking_in(GoalId::Eat));
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
                (GoalId::Idle, 0.0),
                (GoalId::Play, 0.0),
                (GoalId::Sleep, 0.0),
                (GoalId::Salvage, 0.0),
                (GoalId::Work, 0.0),
                (GoalId::Collect, 0.0),
                (GoalId::Bury, 0.0),
                (GoalId::Smelt, 0.0),
                (GoalId::Haul, 0.0),
                (GoalId::Farm, 0.0),
                (GoalId::Repair, 0.0),
                (GoalId::Cook, 0.0),
                (GoalId::Recover, 0.0),
                (GoalId::Care, 0.0),
                (GoalId::Flee, 0.0),
                (GoalId::Hunt, 0.0),
                (GoalId::Prowl, 0.0),
                (GoalId::Depart, 0.0),
                (GoalId::Fish, 0.0),
                (GoalId::Chat, 0.0),
                (GoalId::Decorate, 0.0),
                (GoalId::Clean, 0.0),
                (GoalId::DogCare, 0.0),
                (GoalId::Follow, 0.0),
                (GoalId::Defecate, 0.0),
                (GoalId::Provision, 0.0)
            ]
        );
    }
}
