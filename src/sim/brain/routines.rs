//! The routines that exist.

use crate::save::{invalid, Invalid};
use crate::sim::biology::{Biology, ProcessId, Stats};
use crate::sim::clock::HOUR;
use crate::sim::inventory::Inventory;
use crate::sim::item::ItemKind;
use crate::sim::job::{is_bedtime, JobKind};

use super::goal::{GoalId, Goals};
use super::memory::SALVAGE_ORDER;
use super::routine::{RoutineCtx, RoutineExecutor};

/// Hunger at which a human decides it is time to eat.
pub const PECKISH: f32 = 60.0;

/// Hunger a human has to be brought down to before it stops thinking about
/// food. Well below [`PECKISH`], which is the point — see [`NeedRoutine`].
pub const SATED: f32 = 25.0;

/// Thirst at which a human decides it is time for a drink. The same line as
/// [`PECKISH`]: thirst is on the same scale and only rises faster
/// ([`THIRST_PER_SECOND`](crate::sim::biology::thirst::THIRST_PER_SECOND)).
pub const THIRSTY: f32 = 60.0;

/// Thirst a human has to be brought down to before it stops thinking about
/// drinking.
pub const QUENCHED: f32 = 25.0;

/// Bladder at which a human decides it is time for the toilet.
pub const BURSTING: f32 = 70.0;

/// Bladder a human has to be brought down to before it stops thinking about
/// the toilet. A toilet empties it, so anything above zero will do; low,
/// because a bladder refilling from a recent drink should not send somebody
/// straight back.
pub const RELIEVED: f32 = 10.0;

/// Boredom at which a human decides it is time to find something to do.
///
/// The same line as [`PECKISH`] — every need commits on the same scale — but
/// it takes far longer to get here: fun drains over
/// [`HOURS_TO_BORED`](crate::sim::biology::fun::HOURS_TO_BORED), twice what an
/// empty stomach takes.
pub const BORED: f32 = 60.0;

/// Boredom a human has to be brought down to before it stops looking for
/// something to do. One go at something is worth more than the gap between
/// the two, so a session ends the wanting rather than taking the edge off it.
pub const AMUSED: f32 = 20.0;

/// Points of a need per point of priority. Every need is 0-100 and the floor
/// under everything ([`StayBusyRoutine`]) is 0.1, so a human who has only just
/// committed to eating (hunger 25-ish) already outranks wandering five times
/// over, and a starving one is at 2. One number for every need, so that how
/// urgent hunger is and how urgent thirst is can be compared at all.
const NEED_PER_PRIORITY: f32 = 50.0;

/// A need worse than this is **urgent**: its unit hurries to see to it,
/// more the worse it gets, flat out at 100 — starving, parched, about to
/// wet itself. The line where a need takes a worker off its work
/// ([`CALLED_AWAY`]), and for the same reason: past it, the need is what
/// matters.
pub const URGENT: f32 = CALLED_AWAY;

/// What [`Need::urgent_at`] says of a need that is never worth hurrying for:
/// boredom, which does a body no harm.
pub const NEVER_URGENT: f32 = f32::INFINITY;

/// A need a [`NeedRoutine`] keeps met: which stat it watches, the process
/// that drives that stat, which goal meets it, and where it commits and lets
/// go.
#[derive(Clone, Copy, Debug)]
pub struct Need {
    /// The routine's name, for the brains menu.
    pub routine: &'static str,
    /// What the brains menu says while it is committed: `hungry yes`.
    pub feeling: &'static str,
    pub stat: fn(&Stats) -> f32,
    /// The process behind the stat. Switched off, the need is not wanted at
    /// all: a stat that cannot move is not one a goal could ever meet.
    pub process: ProcessId,
    pub goal: GoalId,
    /// The stat at which it commits to the goal...
    pub commit_at: f32,
    /// ...and the stat it has to be brought down to before letting go.
    pub release_at: f32,
    /// The stat past which seeing to it is urgent, and its unit hurries:
    /// [`URGENT`], or [`NEVER_URGENT`].
    pub urgent_at: f32,
}

/// Keep fed: hunger, met by eating.
pub const HUNGER: Need = Need {
    routine: "keep fed",
    feeling: "hungry",
    stat: Stats::hunger,
    process: ProcessId::Hunger,
    goal: GoalId::Eat,
    commit_at: PECKISH,
    release_at: SATED,
    urgent_at: URGENT,
};

/// Keep hydrated: thirst, met by drinking.
pub const THIRST: Need = Need {
    routine: "keep hydrated",
    feeling: "thirsty",
    stat: Stats::thirst,
    process: ProcessId::Thirst,
    goal: GoalId::Drink,
    commit_at: THIRSTY,
    release_at: QUENCHED,
    urgent_at: URGENT,
};

/// Stay comfortable: bladder, met by using the toilet.
pub const BLADDER: Need = Need {
    routine: "stay comfortable",
    feeling: "bursting",
    stat: Stats::bladder,
    process: ProcessId::Bladder,
    goal: GoalId::Relieve,
    commit_at: BURSTING,
    release_at: RELIEVED,
    urgent_at: URGENT,
};

/// Keep entertained: boredom, met by having a go at something.
///
/// The stat behind it is [`Stats::fun`], which falls where every other need
/// rises — [`Stats::boredom`] is that number turned over, so this need
/// commits and releases on the same scale as the rest.
pub const BOREDOM: Need = Need {
    routine: "keep entertained",
    feeling: "bored",
    stat: Stats::boredom,
    process: ProcessId::Fun,
    goal: GoalId::Play,
    commit_at: BORED,
    release_at: AMUSED,
    urgent_at: NEVER_URGENT,
};

/// Maps a need onto how badly the goal that meets it is wanted.
///
/// **With hysteresis.** It commits at [`Need::commit_at`] and stays committed
/// until the stat is down to [`Need::release_at`], so a stat hovering at a
/// single threshold does not make a human walk half way to the fridge and
/// turn round — and a meal that only takes the edge off sends it back for a
/// second one rather than leaving it one step from peckish.
///
/// One routine for every need rather than one per need: keeping fed, keeping
/// hydrated and staying comfortable are the same decision about different
/// numbers, and the numbers are what [`Need`] holds.
#[derive(Clone, Copy, Debug)]
pub struct NeedRoutine {
    need: &'static Need,
    committed: bool,
}

impl NeedRoutine {
    pub const fn new(need: &'static Need) -> NeedRoutine {
        NeedRoutine {
            need,
            committed: false,
        }
    }

    /// Whether it has committed to its goal — the one thing it remembers
    /// from one tick to the next, and so the one thing a save keeps.
    pub(crate) fn is_committed(&self) -> bool {
        let NeedRoutine { need: _, committed } = *self;
        committed
    }

    pub(crate) fn set_committed(&mut self, committed: bool) {
        self.committed = committed;
    }
}

impl RoutineExecutor for NeedRoutine {
    fn name(&self) -> &'static str {
        self.need.routine
    }

    fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        // Nothing to keep met: no body, or not this process in it.
        let Some(biology) = ctx.biology.filter(|biology| biology.is_running(self.need.process)) else {
            self.committed = false;
            return;
        };
        let level = (self.need.stat)(biology.stats());
        if level >= self.need.commit_at {
            self.committed = true;
        } else if level <= self.need.release_at {
            self.committed = false;
        }
        if self.committed {
            let mut priority = level / NEED_PER_PRIORITY;
            if self.need.goal == GoalId::Play && is_bedtime(ctx.job, ctx.think.clock) && biology.is_running(ProcessId::Energy) {
                // Entertainment can fill time if no bed is usable, but must
                // never keep somebody up or wake them during the night —
                // their own night, which a night watch keeps by day.
                priority = priority.min(SLEEPY / NEED_PER_PRIORITY / 2.0);
            }
            if self.need.goal == GoalId::Play && ctx.job.is_some_and(|job| job.is_due(ctx.think.clock)) {
                // Work is no fun, and a worker does not go off for a go at
                // something while its shift is due: the TV it may watch is
                // its own, at home across the district as often as not. With
                // nothing to do at work, though, it may.
                priority = priority.min(BORED_AT_WORK);
            }
            goals.raise_to(self.need.goal, priority);
            goals.hurry(self.need.goal, urgency(level, self.need.urgent_at));
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(self.need.feeling, if self.committed { "yes" } else { "no" }.to_string())]
    }
}

/// How urgent a need at `level` is, 0 to 1: nothing up to `urgent_at`, then
/// rising in a straight line to all there is at 100.
fn urgency(level: f32, urgent_at: f32) -> f32 {
    // First, or `NEVER_URGENT` would be infinity over infinity.
    if level <= urgent_at {
        return 0.0;
    }
    ((level - urgent_at) / (100.0 - urgent_at)).min(1.0)
}

/// The most a go at something is wanted while its worker's shift is due:
/// boredom's commit level, under [`WORKING`] — so it never takes a worker off
/// its work, and still comes before wandering about when the work is held
/// off. Boredom does a body no harm.
const BORED_AT_WORK: f32 = BORED / NEED_PER_PRIORITY;

/// Tiredness at which a human decides to go to bed **in the daytime**. High:
/// somebody has to be properly worn out to give up an afternoon.
///
/// A waking day is sixteen world hours
/// ([`HOURS_TO_EXHAUSTED`](crate::sim::biology::energy::HOURS_TO_EXHAUSTED)),
/// so this is about twelve of them.
pub const EXHAUSTED: f32 = 75.0;

/// Tiredness floor used to prioritize sleep **at night**: from
/// [`BEDTIME`](crate::sim::clock::BEDTIME) to
/// [`GETTING_UP`](crate::sim::clock::GETTING_UP). Bedtime applies even below
/// this level, so a rested human still goes to bed and stays there.
pub const SLEEPY: f32 = 30.0;

/// Tiredness a human has to be brought down to before it gets up — which
/// still waits for morning if it is night, see [`SleepRoutine`].
pub const RESTED: f32 = 10.0;

/// Tiredness at which a human will sleep in **any** free bed rather than only
/// its own. A unit goes to its own bed by default; it takes one at random only
/// when it cannot go on, which is a long way past [`EXHAUSTED`] — by day, two
/// and a half more world hours awake.
pub const CRITICALLY_TIRED: f32 = 90.0;

/// Sleep: tiredness and the time of day, met by going to bed.
///
/// A routine type of its own rather than a [`Need`], because a need reads a
/// stat and this reads the clock too. Otherwise the same decision, and with
/// the same hysteresis — commit at one level, let go at a lower.
///
/// * **By day** it commits at [`EXHAUSTED`] and lets go at [`RESTED`]: a nap
///   when somebody cannot go on.
/// * **By night** it commits regardless of tiredness, and **does not let go
///   until it is day**. Night is the unit's own: the clock's
///   ([`BEDTIME`](crate::sim::clock::BEDTIME) to
///   [`GETTING_UP`](crate::sim::clock::GETTING_UP)), or for somebody with a
///   job the eight hours before its shift's morning
///   ([`Shift::is_bedtime`](crate::sim::job::Shift::is_bedtime)), which for
///   office hours is the same thing. The priority has a floor of [`SLEEPY`] so a rested
///   human goes to bed and stays there against wandering and entertainment.
///   Food, water and a bursting bladder can still get somebody up, and they
///   come back afterwards.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SleepRoutine {
    committed: bool,
}

impl SleepRoutine {
    pub const fn new() -> SleepRoutine {
        SleepRoutine { committed: false }
    }

    /// Whether it has committed to sleeping — all a save keeps of it.
    pub(crate) fn is_committed(&self) -> bool {
        let SleepRoutine { committed } = *self;
        committed
    }

    pub(crate) fn set_committed(&mut self, committed: bool) {
        self.committed = committed;
    }
}

impl RoutineExecutor for SleepRoutine {
    fn name(&self) -> &'static str {
        "get some sleep"
    }

    fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        // Nothing to rest: no body, or no tiring in it.
        let Some(biology) = ctx.biology.filter(|biology| biology.is_running(ProcessId::Energy)) else {
            self.committed = false;
            return;
        };
        let tiredness = biology.stats().tiredness();
        // Their own night: the clock's, unless a shift moves it
        // (`Shift::is_bedtime` — the night watch sleeps the afternoon).
        let night = is_bedtime(ctx.job, ctx.think.clock);

        if night || tiredness >= EXHAUSTED {
            self.committed = true;
        } else if tiredness <= RESTED && !night {
            self.committed = false;
        }
        if self.committed {
            let floor = if night { SLEEPY } else { 0.0 };
            goals.raise_to(GoalId::Sleep, tiredness.max(floor) / NEED_PER_PRIORITY);
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("sleepy", if self.committed { "yes" } else { "no" }.to_string())]
    }
}

/// How much [`StayBusyRoutine`] wants a unit wandering.
pub const BUSY: f32 = 0.1;

/// The floor the list rests on: wander, a little, always. A unit with no
/// pressing need is not standing still.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StayBusyRoutine;

impl RoutineExecutor for StayBusyRoutine {
    fn name(&self) -> &'static str {
        "stay busy"
    }

    fn arrange(&mut self, _ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        goals.raise_to(GoalId::Wander, BUSY);
    }
}

/// How much [`OrdersRoutine`] wants an order carried out: over wandering,
/// under every need committed to and under a night in bed (`SLEEPY / 50`,
/// 0.6). An order is an errand, got round to when nothing more pressing is.
pub const ORDERED: f32 = 0.5;

/// Do what it was told — so far, salvage a heap of scrap
/// ([`crate::sim::Command::Salvage`]). Wants
/// [`SalvageGoal`](super::goals::SalvageGoal) for as long as the order is
/// remembered; the goal is what lets it go.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct OrdersRoutine;

impl RoutineExecutor for OrdersRoutine {
    fn name(&self) -> &'static str {
        "do as told"
    }

    fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        if ctx.memory.get(SALVAGE_ORDER).is_some() {
            goals.raise_to(GoalId::Salvage, ORDERED);
        }
    }
}

/// How much [`WorkRoutine`] wants a worker at its desk while the shift is
/// due.
///
/// **Over every need at the point it commits** — hunger, thirst and boredom
/// at 1.2 ([`PECKISH`] / 50), a bursting bladder at 1.4 — so somebody only
/// just peckish waits for the end of the shift, or an hour's break between
/// two stretches does not happen at all. **Under every need once it is
/// urgent**: past 75 any of them is worth more than 1.5 and takes the worker
/// away from the desk, and work has it back once it is seen to. A nap at
/// [`EXHAUSTED`] (75, so 1.5 exactly) ties, and the tie goes to sleep: ties
/// go to the lower [`GoalId`], and work is the last.
pub const WORKING: f32 = 1.5;

/// How urgent getting to work is the moment a shift starts with its worker
/// not there yet ([`WorkRoutine`]): half a hurry, rising to a run
/// [`LATE_BY`] later.
pub const LATE: f32 = 0.5;

/// How late, in world seconds, a worker has to be to run to work flat out.
pub const LATE_BY: f32 = HOUR;

/// How near its workplace, in cells either way, a worker has to stand to be
/// there: beside it, where a desk is worked from.
const AT_WORK: i32 = 1;

/// Go to work: want the job's goal ([`JobKind::goal`](crate::sim::job::JobKind::goal)
/// — [`GoalId::Work`] at a desk, [`GoalId::Collect`] for a scrap collector)
/// at [`WORKING`] for as long as the shift is due
/// ([`Job::is_due`](crate::sim::job::Job::is_due) — its hours on its
/// days, and the commute before them).
///
/// It reads the clock and the job and no stat, so it needs no hysteresis:
/// nothing the goal does moves when the shift is. What it remembers is only
/// what it decided last tick, for the brains menu and a save.
///
/// **Late is urgent.** A worker whose job is kept in one place — a desk, a
/// guard post, a control panel: the jobs done by [`GoalId::Work`] — and who
/// is not beside it once the shift's hours have begun ([`Shift::is_working_hours`](crate::sim::job::Shift::is_working_hours))
/// hurries there, [`LATE`] at first and flat out [`LATE_BY`] later; and so
/// does one coming back from a need that took it away. Nothing here
/// remembers whether it got there today, so a job worked on the move — a
/// scrap collector's, a courier's — is never late: its worker is away from
/// its workplace for most of the shift by design.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WorkRoutine {
    on_shift: bool,
}

impl WorkRoutine {
    pub const fn new() -> WorkRoutine {
        WorkRoutine { on_shift: false }
    }

    /// Whether it wanted work last tick — all a save keeps of it.
    pub(crate) fn is_on_shift(&self) -> bool {
        let WorkRoutine { on_shift } = *self;
        on_shift
    }

    pub(crate) fn set_on_shift(&mut self, on_shift: bool) {
        self.on_shift = on_shift;
    }
}

impl RoutineExecutor for WorkRoutine {
    fn name(&self) -> &'static str {
        "go to work"
    }

    fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        let due = ctx.job.filter(|job| job.is_due(ctx.think.clock));
        self.on_shift = due.is_some();
        if let Some(job) = due {
            goals.raise_to(job.kind.goal(), WORKING);
            let away = ctx.body.center_position().chebyshev_distance(job.workplace) > AT_WORK;
            if job.kind.goal() == GoalId::Work && away && job.shift.is_working_hours(ctx.think.clock) {
                let late = ctx.think.clock.hours() - job.shift.start() as f32;
                goals.hurry(GoalId::Work, LATE + (1.0 - LATE) * late * HOUR / LATE_BY);
            }
        }
        // An undertaker with a body in hand sees it to the fire, shift over
        // or not: it is not taken home, and nothing else may be done with
        // the hand full of it.
        if let Some(job) = ctx.job
            && job.kind == JobKind::Undertaker
            && ctx.inventory.and_then(Inventory::hand) == Some(ItemKind::Body)
        {
            goals.raise_to(GoalId::Bury, WORKING);
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("on shift", if self.on_shift { "yes" } else { "no" }.to_string())]
    }
}

/// A need no worse than this — 85% met — is one a worker sets out on a trip
/// into the wasteland with ([`ReadyRoutine`]): hunger, thirst and the bladder
/// at most 15.
pub const READY: f32 = 15.0;

/// Once a worker has stopped to get ready, it sees to every need worse than
/// this — 95% met — while it is about it, not only the ones past [`READY`]:
/// thirst at 12 is past 15 by the end of the meal, and a second drink after
/// the toilet is a bladder half full again before the trip is under way.
pub const TOPPED_UP: f32 = 5.0;

/// How much [`ReadyRoutine`] wants the first need on its list seen to before
/// a trip out: over [`WORKING`], so the trip waits for it, and under any need
/// past 80. Each need after it on the list is [`IN_TURN`] less, all of them
/// still over work.
pub const GETTING_READY: f32 = 1.6;

/// How far apart [`ReadyRoutine`] puts the needs on its list, so that they
/// are seen to in the list's order rather than in the goals'.
const IN_TURN: f32 = 0.01;

/// A need worse than this takes a worker off its work ([`WORKING`] is the
/// priority of a need at it): one that does, on a job worked out in the
/// wasteland, has [`ReadyRoutine`] see to every need while it is about it.
const CALLED_AWAY: f32 = WORKING * NEED_PER_PRIORITY;

/// What [`ReadyNeed::top_up`] says of a need [`ReadyRoutine`] never sees to:
/// boredom, which never takes a worker off its shift ([`BORED_AT_WORK`]) and
/// so never calls one back from a trip. Seen to before every trip, it sent a
/// worker home to its TV across the district, and by the time it was back
/// the rest were short again: it was never ready.
const NOT_BEFORE_A_TRIP: f32 = f32::INFINITY;

/// A need [`ReadyRoutine`] sees to before a trip out, and how far up it tops
/// it once it has stopped to get ready.
#[derive(Clone, Copy, Debug)]
pub struct ReadyNeed {
    pub need: &'static Need,
    /// Seen to while worse than this, once stopped: [`TOPPED_UP`], or
    /// [`READY`] for a need that does not stay topped up long enough to be
    /// worth it.
    pub top_up: f32,
}

/// The needs a worker sees to before it sets out, in the order it sees to
/// them: a drink first, since its water reaches the bladder over half an hour
/// ([`ARRIVES_OVER`](crate::sim::biology::bladder::ARRIVES_OVER)); a meal; the
/// toilet last, so that it leaves empty with the drink already in it. **A go
/// at something keeps its place on the list** — a save's bits are its places
/// — but is never seen to ([`NOT_BEFORE_A_TRIP`]).
///
/// **Not tiredness**: nothing but a night's sleep is any good for that, and a
/// worker who got up rested ends its shift short of [`EXHAUSTED`] by design
/// (the `jobs` skill), so tiredness never calls one back from a trip.
pub const READY_NEEDS: [ReadyNeed; 4] = [
    ReadyNeed { need: &THIRST, top_up: TOPPED_UP },
    ReadyNeed { need: &HUNGER, top_up: TOPPED_UP },
    ReadyNeed { need: &BOREDOM, top_up: NOT_BEFORE_A_TRIP },
    ReadyNeed { need: &BLADDER, top_up: TOPPED_UP },
];

/// Get ready to go out: before every trip out into the wasteland, see to
/// every need on [`READY_NEEDS`] until it is [`READY`], and only then set out
/// — so that nothing calls a worker back in from the far end of a trip, an
/// hour's walk from the nearest fridge or toilet. For a job worked on trips
/// out ([`JobKind::goes_out_for`]): the scrap collector's.
///
/// * **When**: while the shift is due, in the district, with room in the
///   pack — about to go out, then, not on the way to the bin with a packful
///   — and either **beside the bin**, where every trip starts
///   (`CollectGoal` goes there first) and not at home across the district, a
///   long walk from the way out; or **back in with a trip half done**,
///   called in by a need, where the fridge it came in for is; or **called
///   away by a need** anywhere in the district, ready or not — one past
///   [`CALLED_AWAY`], which takes it off its work whatever this routine
///   says: it is stopping for that one anyway, and without the rest seen to
///   as well the next calls it back an hour later. A farmer living across
///   the district from the farm turned back home for one need after
///   another and never got there. Started, it goes on wherever the fridge
///   and the toilet are. Out in the wasteland there is nothing to see to
///   anything with.
/// * **What**: when any need is worse than [`READY`], a list, taken there
///   and then, of every need worse than its top-up level
///   ([`ReadyNeed::top_up`]: [`TOPPED_UP`]; fun never) whose process is
///   running and whose goal is not held off — a need nothing in reach can
///   meet (no fridge) is not waited for — raised over [`WORKING`] in the
///   list's order. A need leaves the list once it is topped up, and
///   **nothing joins it meanwhile**: thirst creeping up during the meal does
///   not turn the worker round on its way to the toilet. When the list is
///   empty a fresh one is taken if anything is worse than ready again; none
///   is **ready**.
/// * **Ready lasts** until it has gone out or the shift is over, so the walk
///   out to the gate does not send it back for the thirst it works up on
///   the way.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ReadyRoutine {
    /// Seen to, and setting out.
    ready: bool,
    /// A bit for each need on [`READY_NEEDS`] still to be seen to.
    pending: u8,
}

impl ReadyRoutine {
    pub const fn new() -> ReadyRoutine {
        ReadyRoutine { ready: false, pending: 0 }
    }

    /// Whether it is seeing to a need before a trip out right now.
    pub fn is_getting_ready(&self) -> bool {
        self.pending != 0
    }

    /// Whether it is ready to go out, and the needs it is still seeing to —
    /// all a save keeps of it.
    pub(crate) fn state(&self) -> (bool, u8) {
        let ReadyRoutine { ready, pending } = *self;
        (ready, pending)
    }

    pub(crate) fn set_state(&mut self, ready: bool, pending: u32) -> Result<(), Invalid> {
        if pending >> READY_NEEDS.len() != 0 {
            return invalid(format!("getting ready: no needs numbered {pending:#b}"));
        }
        let pending = pending as u8;
        if ready && pending != 0 {
            return invalid("ready to go out with needs still to see to");
        }
        *self = ReadyRoutine { ready, pending };
        Ok(())
    }

    /// A bit for each need on [`READY_NEEDS`] worse than `level` says,
    /// whose process is running, and whose goal is not held off.
    fn worse_than(level: impl Fn(&ReadyNeed) -> f32, biology: &Biology, goals: &Goals) -> u8 {
        let mut worse = 0;
        for (i, ready) in READY_NEEDS.iter().enumerate() {
            let need = ready.need;
            if biology.is_running(need.process) && (need.stat)(biology.stats()) > level(ready) && goals.cooldown(need.goal) == 0 {
                worse |= 1 << i;
            }
        }
        worse
    }
}

impl RoutineExecutor for ReadyRoutine {
    fn name(&self) -> &'static str {
        "get ready to go out"
    }

    fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        let due = ctx.job.filter(|job| job.is_due(ctx.think.clock));
        let (Some(job), Some(biology)) = (due, ctx.biology) else {
            *self = ReadyRoutine::new();
            return;
        };
        let Some(item) = job.kind.goes_out_for() else {
            *self = ReadyRoutine::new();
            return;
        };
        let here = ctx.body.center_position();
        if !ctx.think.map.contains(here) {
            // Gone out: ready for the next time only when back in.
            *self = ReadyRoutine::new();
            return;
        }
        let Some(pack) = ctx.inventory.filter(|pack| pack.room_for(item)) else {
            // A packful, on its way to the bin: stored first.
            return;
        };
        let carrying = pack.count(item) > 0;
        let called_away = Self::worse_than(|ready| ready.top_up.max(CALLED_AWAY), biology, goals) != 0;
        if called_away && self.pending == 0 {
            // Stopped by a need, ready or not: everything else while it is
            // about it.
            self.ready = false;
        }
        if self.ready || (self.pending == 0 && !carrying && !called_away && here.manhattan_distance(job.workplace) > 1) {
            // Ready, and on the way out; or not at the start of a trip yet —
            // beside the bin, where the job's goal goes first, back in with
            // a trip half done, or called away by a need.
            return;
        }
        // A need stays on the list until it is topped up; a fresh list is
        // taken when anything is worse than ready, and tops up everything.
        let short = Self::worse_than(|ready| ready.top_up, biology, goals);
        self.pending &= short;
        if self.pending == 0 && Self::worse_than(|ready| ready.top_up.max(READY), biology, goals) != 0 {
            self.pending = short;
        }
        if self.pending == 0 {
            self.ready = true;
            ctx.think.log.push(format!("{} is ready to go out for {}", ctx.body.uid(), item.name()));
            return;
        }
        for (i, ready) in READY_NEEDS.iter().enumerate() {
            if self.pending & (1 << i) != 0 {
                goals.raise_to(ready.need.goal, GETTING_READY - i as f32 * IN_TURN);
            }
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let seeing_to: Vec<&str> = READY_NEEDS
            .iter()
            .enumerate()
            .filter(|(i, _)| self.pending & (1 << i) != 0)
            .map(|(_, ready)| ready.need.goal.name())
            .collect();
        vec![
            ("ready to go out", if self.ready { "yes" } else { "no" }.to_string()),
            ("seeing to", if seeing_to.is_empty() { "nothing".to_string() } else { seeing_to.join(", ") }),
        ]
    }
}

/// Raises one goal to a priority read from a shared dial, so a test can turn a
/// goal up and down from outside the brain — the brain's own tests drive
/// handovers with it.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct Dial {
    pub goal: GoalId,
    pub level: std::sync::Arc<std::sync::Mutex<f32>>,
}

#[cfg(test)]
impl RoutineExecutor for Dial {
    fn name(&self) -> &'static str {
        "dial"
    }

    fn arrange(&mut self, _ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        goals.raise_to(self.goal, *self.level.lock().unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Point, Size, FLOOR};
    use crate::sim::entity::{Body, Think};
    use crate::sim::facilities::Facilities;
    use crate::sim::log::Log;
    use crate::sim::occupancy::Occupancy;
    use crate::sim::biology::Biology;
    use crate::sim::clock::Clock;
    use crate::sim::uid::{EntityType, Uid};

    /// The priority `routine` gives its goal for a human whose `stats` these are.
    fn priority(routine: &mut NeedRoutine, stats: Stats) -> f32 {
        priority_in(routine, &Biology::new(stats))
    }

    fn priority_in(routine: &mut NeedRoutine, biology: &Biology) -> f32 {
        let map = Map::new(Size::new(2, 2), FLOOR);
        let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
        let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, crate::sim::clock::Clock::after_watching(0.0));
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(biology),
                memory: &crate::sim::brain::memory::Memory::new(),
                job: None,
                inventory: None,
            },
            &mut goals,
        );
        goals.priority(routine.need.goal)
    }

    fn eat_priority(routine: &mut NeedRoutine, hunger: f32) -> f32 {
        priority(routine, Stats::calm().with_hunger(hunger))
    }

    fn drink_priority(routine: &mut NeedRoutine, thirst: f32) -> f32 {
        priority(routine, Stats::calm().with_thirst(thirst))
    }

    #[test]
    fn a_fed_human_does_not_want_to_eat() {
        assert_eq!(eat_priority(&mut NeedRoutine::new(&HUNGER), 10.0), 0.0);
    }

    #[test]
    fn a_hungry_human_wants_to_eat_more_than_it_wants_to_wander() {
        assert!(eat_priority(&mut NeedRoutine::new(&HUNGER), 70.0) > BUSY);
    }

    #[test]
    fn hunger_hovering_at_the_threshold_does_not_flip_the_decision() {
        let mut routine = NeedRoutine::new(&HUNGER);
        // Not hungry enough to start...
        assert_eq!(eat_priority(&mut routine, PECKISH - 1.0), 0.0);
        // ...hungry enough...
        assert!(eat_priority(&mut routine, PECKISH) > BUSY);
        // ...and dipping back under the line is not enough to stop.
        assert!(eat_priority(&mut routine, PECKISH - 1.0) > BUSY);
        assert!(eat_priority(&mut routine, SATED + 1.0) > BUSY);
        // Properly fed is.
        assert_eq!(eat_priority(&mut routine, SATED), 0.0);
        assert_eq!(eat_priority(&mut routine, SATED + 1.0), 0.0);
    }

    #[test]
    fn thirst_hovering_at_the_threshold_does_not_flip_the_decision() {
        let mut routine = NeedRoutine::new(&THIRST);
        assert_eq!(drink_priority(&mut routine, THIRSTY - 1.0), 0.0);
        assert!(drink_priority(&mut routine, THIRSTY) > BUSY);
        assert!(drink_priority(&mut routine, THIRSTY - 1.0) > BUSY);
        assert!(drink_priority(&mut routine, QUENCHED + 1.0) > BUSY);
        assert_eq!(drink_priority(&mut routine, QUENCHED), 0.0);
        assert_eq!(drink_priority(&mut routine, QUENCHED + 1.0), 0.0);
    }

    #[test]
    fn bladder_hovering_at_the_threshold_does_not_flip_the_decision() {
        let bladder = |routine: &mut NeedRoutine, level| priority(routine, Stats::calm().with_bladder(level));
        let mut routine = NeedRoutine::new(&BLADDER);
        assert_eq!(bladder(&mut routine, BURSTING - 1.0), 0.0);
        assert!(bladder(&mut routine, BURSTING) > BUSY);
        assert!(bladder(&mut routine, RELIEVED + 1.0) > BUSY);
        assert_eq!(bladder(&mut routine, RELIEVED), 0.0);
        assert_eq!(bladder(&mut routine, BURSTING - 1.0), 0.0);
    }

    /// How urgent `routine` makes its goal for a human whose `stats` these are.
    fn urgency_of(routine: &mut NeedRoutine, stats: Stats) -> f32 {
        let map = Map::new(Size::new(2, 2), FLOOR);
        let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
        let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, Clock::after_watching(0.0));
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(&Biology::new(stats)),
                memory: &crate::sim::brain::memory::Memory::new(),
                job: None,
                inventory: None,
            },
            &mut goals,
        );
        goals.urgency(routine.need.goal)
    }

    #[test]
    fn a_need_is_urgent_only_past_seventy_five_and_most_urgent_at_its_worst() {
        let needs: [(&'static Need, fn(f32) -> Stats); 3] = [
            (&HUNGER, |level| Stats::calm().with_hunger(level)),
            (&THIRST, |level| Stats::calm().with_thirst(level)),
            (&BLADDER, |level| Stats::calm().with_bladder(level)),
        ];
        for (need, at) in needs {
            let mut routine = NeedRoutine::new(need);
            assert_eq!(urgency_of(&mut routine, at(URGENT)), 0.0, "{}: committed, in no hurry", need.routine);
            assert!((urgency_of(&mut routine, at(87.5)) - 0.5).abs() < 1e-4, "{}: half way to its worst", need.routine);
            assert_eq!(urgency_of(&mut routine, at(100.0)), 1.0, "{}: at its worst", need.routine);
        }
        let bored = Stats::calm().with_fun(0.0);
        assert!(priority(&mut NeedRoutine::new(&BOREDOM), bored) > 0.0);
        assert_eq!(urgency_of(&mut NeedRoutine::new(&BOREDOM), bored), 0.0, "nobody runs for the TV");
    }

    #[test]
    fn a_need_whose_process_is_switched_off_is_not_wanted_and_lets_go_of_a_commitment() {
        let mut biology = Biology::new(Stats::calm().with_hunger(90.0));
        let mut routine = NeedRoutine::new(&HUNGER);
        assert!(priority_in(&mut routine, &biology) > BUSY);

        biology.set_running(ProcessId::Hunger, false);
        assert_eq!(priority_in(&mut routine, &biology), 0.0);
        assert_eq!(routine.debug_fields(), [("hungry", "no".to_string())]);

        // Only the one switched off.
        assert!(priority_in(&mut NeedRoutine::new(&THIRST), &Biology::new(Stats::calm().with_thirst(90.0))) > BUSY);
    }

    #[test]
    fn boredom_hovering_at_the_threshold_does_not_flip_the_decision() {
        let boredom = |routine: &mut NeedRoutine, level: f32| {
            priority(routine, Stats::calm().with_fun(100.0 - level))
        };
        let mut routine = NeedRoutine::new(&BOREDOM);
        assert_eq!(boredom(&mut routine, BORED - 1.0), 0.0);
        assert!(boredom(&mut routine, BORED) > BUSY);
        assert!(boredom(&mut routine, AMUSED + 1.0) > BUSY);
        assert_eq!(boredom(&mut routine, AMUSED), 0.0);
        assert_eq!(boredom(&mut routine, BORED - 1.0), 0.0);
    }

    #[test]
    fn a_bored_human_wants_something_to_do_more_than_it_wants_to_wander() {
        let bored = Stats::calm().with_fun(100.0 - BORED);
        assert!(priority(&mut NeedRoutine::new(&BOREDOM), bored) > BUSY);
        assert_eq!(
            priority(&mut NeedRoutine::new(&BOREDOM), Stats::calm().with_fun(100.0)),
            0.0,
            "somebody having a great time is not looking for something to do"
        );
    }

    /// Being as bored as you are hungry is being equally in need of seeing to
    /// — which is only true because boredom is fun read the same way up as
    /// every other need.
    #[test]
    fn boredom_and_hunger_that_are_equally_bad_are_equally_urgent() {
        let stats = Stats::calm().with_hunger(80.0).with_fun(20.0);
        assert_eq!(
            priority(&mut NeedRoutine::new(&HUNGER), stats),
            priority(&mut NeedRoutine::new(&BOREDOM), stats)
        );
    }

    #[test]
    fn each_need_raises_its_own_goal_and_reads_only_its_own_stat() {
        let starving_but_quenched = Stats::calm().with_hunger(100.0).with_thirst(0.0);
        let parched_but_fed = Stats::calm().with_hunger(0.0).with_thirst(100.0);
        assert!(priority(&mut NeedRoutine::new(&HUNGER), starving_but_quenched) > BUSY);
        assert_eq!(priority(&mut NeedRoutine::new(&THIRST), starving_but_quenched), 0.0);
        assert!(priority(&mut NeedRoutine::new(&THIRST), parched_but_fed) > BUSY);
        assert_eq!(priority(&mut NeedRoutine::new(&HUNGER), parched_but_fed), 0.0);
    }

    #[test]
    fn hunger_and_thirst_that_are_equally_bad_are_equally_urgent() {
        // One scale for every need, or which one a human sees to first would
        // be an accident of two constants.
        let stats = Stats::calm().with_hunger(80.0).with_thirst(80.0);
        assert_eq!(
            priority(&mut NeedRoutine::new(&HUNGER), stats),
            priority(&mut NeedRoutine::new(&THIRST), stats)
        );
    }

    /// Eight in the morning, which is when a world opens.
    fn day() -> Clock {
        Clock::after_watching(0.0)
    }

    /// Half past midnight.
    fn night() -> Clock {
        Clock::after_watching((16.5 * crate::sim::clock::HOUR / crate::sim::clock::TIME_SCALE) as f64)
    }

    /// The priority `routine` gives sleep for `biology`, at `clock`.
    fn sleep_priority(routine: &mut SleepRoutine, biology: &Biology, clock: Clock) -> f32 {
        sleep_priority_for(routine, biology, clock, None)
    }

    /// ...for somebody with `job`.
    fn sleep_priority_for(
        routine: &mut SleepRoutine,
        biology: &Biology,
        clock: Clock,
        job: Option<&crate::sim::job::Job>,
    ) -> f32 {
        let map = Map::new(Size::new(2, 2), FLOOR);
        let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
        let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, clock);
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(biology),
                memory: &crate::sim::brain::memory::Memory::new(),
                job,
                inventory: None,
            },
            &mut goals,
        );
        goals.priority(GoalId::Sleep)
    }

    /// The night watch keeps its night in the afternoon: in bed from one
    /// whether tired or not, up at nine in the evening, and awake on watch at
    /// two in the morning, when everybody else is asleep.
    #[test]
    fn a_guard_on_the_night_watch_goes_to_bed_in_the_afternoon_and_stays_up_all_night() {
        use crate::sim::job::{Job, WATCHES};
        let job = Job::guard(Point::new(3, 3), WATCHES[2]);
        let mut routine = SleepRoutine::new();
        assert!(sleep_priority_for(&mut routine, &tired(0.0), on(2, 14.0), Some(&job)) > BUSY, "two in the afternoon is its night");
        assert_eq!(sleep_priority_for(&mut routine, &tired(0.0), on(2, 21.5), Some(&job)), 0.0, "up by half past nine");
        let mut routine = SleepRoutine::new();
        assert_eq!(sleep_priority_for(&mut routine, &tired(SLEEPY + 10.0), on(3, 2.0), Some(&job)), 0.0, "on watch at two");
        assert!(sleep_priority_for(&mut SleepRoutine::new(), &tired(0.0), on(3, 2.0), None) > BUSY, "everybody else is in bed");
        let office = Job::office(Point::new(3, 3));
        assert!(sleep_priority_for(&mut SleepRoutine::new(), &tired(0.0), on(3, 2.0), Some(&office)) > BUSY, "and the office");
    }

    fn tired(tiredness: f32) -> Biology {
        Biology::new(Stats::calm().with_stamina(100.0 - tiredness))
    }

    #[test]
    fn somebody_who_is_not_very_tired_does_not_go_to_bed_in_the_afternoon() {
        assert_eq!(sleep_priority(&mut SleepRoutine::new(), &tired(SLEEPY + 10.0), day()), 0.0);
    }

    #[test]
    fn the_same_tiredness_is_reason_enough_to_go_to_bed_at_night() {
        assert!(sleep_priority(&mut SleepRoutine::new(), &tired(SLEEPY + 10.0), night()) > BUSY);
    }

    #[test]
    fn bedtime_sends_even_a_fully_rested_human_to_sleep() {
        let mut routine = SleepRoutine::new();
        assert_eq!(sleep_priority(&mut routine, &tired(0.0), day()), 0.0);
        assert!(sleep_priority(&mut routine, &tired(0.0), night()) > BUSY);
        assert_eq!(sleep_priority(&mut routine, &tired(0.0), day()), 0.0);
    }

    #[test]
    fn somebody_exhausted_goes_to_bed_whatever_time_it_is() {
        assert!(sleep_priority(&mut SleepRoutine::new(), &tired(EXHAUSTED), day()) > BUSY);
    }

    #[test]
    fn tiredness_hovering_at_the_threshold_does_not_flip_the_decision() {
        let mut routine = SleepRoutine::new();
        assert_eq!(sleep_priority(&mut routine, &tired(EXHAUSTED - 1.0), day()), 0.0);
        assert!(sleep_priority(&mut routine, &tired(EXHAUSTED), day()) > BUSY);
        // Dipping back under the line is not enough to get up...
        assert!(sleep_priority(&mut routine, &tired(EXHAUSTED - 1.0), day()) > BUSY);
        assert!(sleep_priority(&mut routine, &tired(RESTED + 1.0), day()) > BUSY);
        // ...being rested is.
        assert_eq!(sleep_priority(&mut routine, &tired(RESTED), day()), 0.0);
        assert_eq!(sleep_priority(&mut routine, &tired(EXHAUSTED - 1.0), day()), 0.0);
    }

    /// The whole point of the night: a rested sleeper stays where they are
    /// until it is light, rather than getting up at one in the morning and
    /// wandering off.
    #[test]
    fn somebody_rested_at_night_stays_in_bed_until_morning() {
        let mut routine = SleepRoutine::new();
        assert!(sleep_priority(&mut routine, &tired(SLEEPY + 5.0), night()) > BUSY);
        let rested = sleep_priority(&mut routine, &tired(0.0), night());
        assert!(rested > BUSY, "{rested}");
        assert_eq!(sleep_priority(&mut routine, &tired(0.0), day()), 0.0, "and gets up when it is day");
    }

    /// Committed at night and still tired at six: they sleep in until they are
    /// rested, rather than being turned out by the clock.
    #[test]
    fn somebody_still_tired_when_the_morning_comes_sleeps_on_until_rested() {
        let mut routine = SleepRoutine::new();
        assert!(sleep_priority(&mut routine, &tired(60.0), night()) > BUSY);
        assert!(sleep_priority(&mut routine, &tired(40.0), day()) > BUSY);
        assert_eq!(sleep_priority(&mut routine, &tired(RESTED), day()), 0.0);
    }

    /// The ordering that gets somebody up for the toilet and back into bed.
    #[test]
    fn a_sleeper_at_night_wants_bed_less_than_a_full_bladder_or_an_empty_stomach() {
        let asleep = sleep_priority(&mut SleepRoutine::new(), &tired(SLEEPY + 5.0), night());
        let bladder = BURSTING / NEED_PER_PRIORITY;
        let hunger = PECKISH / NEED_PER_PRIORITY;
        assert!(asleep < bladder && asleep < hunger, "{asleep} vs {bladder}, {hunger}");
    }

    /// The priority a [`WorkRoutine`] gives work, for somebody with `job`,
    /// at `clock`.
    fn work_priority(routine: &mut WorkRoutine, job: Option<&crate::sim::job::Job>, clock: Clock) -> f32 {
        let map = Map::new(Size::new(2, 2), FLOOR);
        let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
        let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, clock);
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(&Biology::new(Stats::calm())),
                memory: &crate::sim::brain::memory::Memory::new(),
                job,
                inventory: None,
            },
            &mut goals,
        );
        goals.priority(GoalId::Work)
    }

    /// `hours` o'clock on day `day`; day 1 is a Monday.
    fn on(day: u32, hours: f32) -> Clock {
        use crate::sim::clock::{DAY, HOUR, TIME_SCALE};
        let world = (day - 1) as f64 * DAY as f64 + (hours as f64 - 8.0) * HOUR as f64;
        Clock::after_watching(world / TIME_SCALE as f64)
    }

    #[test]
    fn work_is_wanted_on_shift_and_only_by_somebody_with_a_job() {
        let job = crate::sim::job::Job::office(Point::new(3, 3));
        let mut routine = WorkRoutine::new();
        assert_eq!(work_priority(&mut routine, Some(&job), on(1, 10.0)), WORKING);
        assert_eq!(routine.debug_fields(), [("on shift", "yes".to_string())]);
        assert_eq!(work_priority(&mut routine, Some(&job), on(1, 8.75)), WORKING, "on the way at a quarter to nine");
        assert_eq!(work_priority(&mut routine, Some(&job), on(1, 8.0)), 0.0, "too early");
        assert_eq!(work_priority(&mut routine, Some(&job), on(1, 17.5)), 0.0, "home time");
        assert_eq!(work_priority(&mut routine, Some(&job), on(6, 10.0)), 0.0, "Saturday");
        assert_eq!(work_priority(&mut routine, Some(&job), on(7, 10.0)), 0.0, "Sunday");
        assert_eq!(routine.debug_fields(), [("on shift", "no".to_string())]);
        assert_eq!(work_priority(&mut routine, None, on(1, 10.0)), 0.0, "no job, nothing to go to");
    }

    /// The list a [`WorkRoutine`] arranges for somebody with `job` standing
    /// in `cell` at `clock`.
    fn work_goals(job: &crate::sim::job::Job, cell: Point, clock: Clock) -> Goals {
        let map = Map::new(Size::new(8, 8), FLOOR);
        let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
        let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, clock);
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), cell);
        let mut goals = Goals::new();
        WorkRoutine::new().arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(&Biology::new(Stats::calm())),
                memory: &crate::sim::brain::memory::Memory::new(),
                job: Some(job),
                inventory: None,
            },
            &mut goals,
        );
        goals
    }

    #[test]
    fn a_worker_late_for_its_desk_hurries_there_and_the_later_the_faster() {
        let job = crate::sim::job::Job::office(Point::new(5, 5));
        let away = Point::new(0, 0);
        let urgency = |clock: Clock| work_goals(&job, away, clock).urgency(GoalId::Work);
        assert_eq!(urgency(on(1, 8.75)), 0.0, "on the way in good time");
        assert!((urgency(on(1, 9.0)) - LATE).abs() < 1e-3, "late from nine: {}", urgency(on(1, 9.0)));
        assert!((urgency(on(1, 9.5)) - 0.75).abs() < 1e-3, "half an hour late: {}", urgency(on(1, 9.5)));
        assert_eq!(urgency(on(1, 10.0)), 1.0, "an hour late, flat out");
        assert_eq!(urgency(on(1, 16.0)), 1.0, "and no more than that");
        assert_eq!(urgency(on(1, 17.5)), 0.0, "home time");

        // Beside the desk, it is at work; a cell further, it is not.
        assert_eq!(work_goals(&job, Point::new(4, 5), on(1, 10.0)).urgency(GoalId::Work), 0.0);
        assert_eq!(work_goals(&job, Point::new(6, 6), on(1, 10.0)).urgency(GoalId::Work), 0.0);
        assert!(work_goals(&job, Point::new(3, 5), on(1, 10.0)).urgency(GoalId::Work) > 0.0);
    }

    #[test]
    fn a_guard_late_for_its_watch_hurries_and_a_job_worked_on_the_move_never_does() {
        let watch = crate::sim::job::WATCHES[1];
        let guard = crate::sim::job::Job::guard(Point::new(5, 5), watch);
        let start = watch.start() as f32;
        assert_eq!(work_goals(&guard, Point::new(0, 0), on(2, start - 1.0)).urgency(GoalId::Work), 0.0);
        assert!(work_goals(&guard, Point::new(0, 0), on(2, start + 0.25)).urgency(GoalId::Work) > LATE);

        let collector = crate::sim::job::Job::collector(Point::new(5, 5));
        let goals = work_goals(&collector, Point::new(0, 0), on(2, 11.0));
        assert_eq!(goals.priority(GoalId::Collect), WORKING);
        assert_eq!((goals.urgency(GoalId::Collect), goals.urgency(GoalId::Work)), (0.0, 0.0));
    }

    /// A scrap collector's shift is the collect goal, at the same priority
    /// and hours, and never the office's.
    #[test]
    fn a_scrap_collector_on_shift_wants_to_collect_rather_than_to_work() {
        let job = crate::sim::job::Job::collector(Point::new(3, 3));
        let wanted = |clock: Clock| {
            let map = Map::new(Size::new(2, 2), FLOOR);
            let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
            let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, clock);
            let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
            let mut goals = Goals::new();
            WorkRoutine::new().arrange(
                &RoutineCtx {
                    think: &think,
                    body: &body,
                    biology: Some(&Biology::new(Stats::calm())),
                    memory: &crate::sim::brain::memory::Memory::new(),
                    job: Some(&job),
                    inventory: None,
                },
                &mut goals,
            );
            (goals.priority(GoalId::Collect), goals.priority(GoalId::Work))
        };
        assert_eq!(wanted(on(2, 10.0)), (WORKING, 0.0));
        assert_eq!(wanted(on(2, 8.75)), (WORKING, 0.0), "setting off at a quarter to nine");
        assert_eq!(wanted(on(2, 17.5)), (0.0, 0.0), "home time");
        assert_eq!(wanted(on(6, 10.0)), (0.0, 0.0), "Saturday");
    }

    /// Where 1.5 sits: over every need at the point it commits, under every
    /// need once it is urgent.
    #[test]
    fn work_outranks_every_need_at_its_commit_level_and_yields_to_an_urgent_one() {
        for (need, commit) in [("hunger", PECKISH), ("thirst", THIRSTY), ("boredom", BORED), ("bladder", BURSTING)] {
            assert!(WORKING > commit / NEED_PER_PRIORITY, "{need} at {commit} would leave the desk");
            assert!(WORKING < 76.0 / NEED_PER_PRIORITY, "{need} at 76 would not");
        }
        assert!(WORKING > SLEEPY / NEED_PER_PRIORITY && WORKING > ORDERED && WORKING > BUSY);

        // Hunger at 70 and a shift: the shift. At 80: lunch.
        let job = crate::sim::job::Job::office(Point::new(3, 3));
        let ranked = |hunger: f32| {
            let mut goals = Goals::new();
            goals.raise_to(GoalId::Work, work_priority(&mut WorkRoutine::new(), Some(&job), on(2, 11.0)));
            goals.raise_to(GoalId::Eat, priority(&mut NeedRoutine::new(&HUNGER), Stats::calm().with_hunger(hunger)));
            goals.best()
        };
        assert_eq!(ranked(70.0), GoalId::Work);
        assert_eq!(ranked(80.0), GoalId::Eat);
    }

    /// Bored at work is bored at work: while its shift is due, boredom never
    /// beats the job, however bad — and still comes before wandering about,
    /// for when the work is held off. Off shift, or with no job, it is the
    /// need it always was.
    #[test]
    fn boredom_never_takes_a_worker_off_its_shift() {
        let bored = |job: Option<&crate::sim::job::Job>, clock: Clock| {
            let map = Map::new(Size::new(2, 2), FLOOR);
            let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
            let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, clock);
            let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
            let mut goals = Goals::new();
            NeedRoutine::new(&BOREDOM).arrange(
                &RoutineCtx {
                    think: &think,
                    body: &body,
                    biology: Some(&Biology::new(Stats::calm().with_fun(10.0))),
                    memory: &crate::sim::brain::memory::Memory::new(),
                    job,
                    inventory: None,
                },
                &mut goals,
            );
            goals.priority(GoalId::Play)
        };
        let job = crate::sim::job::Job::office(Point::new(3, 3));
        let on_shift = bored(Some(&job), on(2, 11.0));
        assert!(on_shift < WORKING && on_shift > BUSY, "{on_shift}");
        assert_eq!(bored(Some(&job), on(2, 19.0)), 90.0 / NEED_PER_PRIORITY, "after work");
        assert_eq!(bored(None, on(2, 11.0)), 90.0 / NEED_PER_PRIORITY, "no job");
    }

    /// Exhausted is 1.5, the same as work, and the tie goes to the bed: work
    /// is the last goal, and ties go to the first.
    #[test]
    fn a_worker_exactly_exhausted_naps_rather_than_works() {
        let job = crate::sim::job::Job::office(Point::new(3, 3));
        let mut goals = Goals::new();
        goals.raise_to(GoalId::Work, work_priority(&mut WorkRoutine::new(), Some(&job), on(1, 15.0)));
        goals.raise_to(GoalId::Sleep, sleep_priority(&mut SleepRoutine::new(), &tired(EXHAUSTED), on(1, 15.0)));
        assert_eq!(goals.priority(GoalId::Sleep), goals.priority(GoalId::Work));
        assert_eq!(goals.best(), GoalId::Sleep);
    }

    /// A collector whose bin is at (2, 1) of the 4x4 site below.
    fn collector() -> crate::sim::job::Job {
        crate::sim::job::Job::collector(Point::new(2, 1))
    }

    /// Standing beside the bin, where a trip starts; across the site from
    /// it; and out in the wasteland past the site's edge.
    const BESIDE_THE_BIN: Point = Point::new(1, 1);
    const ACROSS_THE_SITE: Point = Point::new(3, 3);
    const OUT_THERE: Point = Point::new(-6, 1);

    /// Hunger, thirst, bladder and boredom at these levels.
    fn needs(hunger: f32, thirst: f32, bladder: f32, boredom: f32) -> Biology {
        Biology::new(Stats::calm().with_hunger(hunger).with_thirst(thirst).with_bladder(bladder).with_fun(100.0 - boredom))
    }

    /// A scrap collector's pack: `scrap` pieces in it.
    fn pack(scrap: u8) -> Inventory {
        let mut pack = Inventory::human();
        for _ in 0..scrap {
            assert!(pack.stow(ItemKind::Scrap));
        }
        pack
    }

    /// Everything a [`ReadyRoutine`] is handed for a tick.
    struct Readiness {
        biology: Biology,
        job: Option<crate::sim::job::Job>,
        pack: Inventory,
        cell: Point,
        clock: Clock,
    }

    impl Readiness {
        /// A collector beside its bin on a Monday morning, nothing in its
        /// pack, with `biology`.
        fn of(biology: Biology) -> Readiness {
            Readiness { biology, job: Some(collector()), pack: pack(0), cell: BESIDE_THE_BIN, clock: on(1, 10.0) }
        }

        /// Arrange `goals` afresh with `routine`, and say what the log was
        /// told.
        fn arrange(&self, routine: &mut ReadyRoutine, goals: &mut Goals) -> Vec<String> {
            let map = Map::new(Size::new(4, 4), FLOOR);
            let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
            let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, self.clock);
            let body = Body::at_cell(Uid::new(EntityType::Human, 1), self.cell);
            goals.clear_priorities();
            routine.arrange(
                &RoutineCtx {
                    think: &think,
                    body: &body,
                    biology: Some(&self.biology),
                    memory: &crate::sim::brain::memory::Memory::new(),
                    job: self.job.as_ref(),
                    inventory: Some(&self.pack),
                },
                goals,
            );
            log.drain()
        }
    }

    /// The goals a [`ReadyRoutine`] raised, highest first.
    fn raised(goals: &Goals) -> Vec<GoalId> {
        goals.ranked().into_iter().filter(|(_, priority)| *priority > 0.0).map(|(goal, _)| goal).collect()
    }

    #[test]
    fn a_collector_about_to_set_out_first_sees_to_every_need_worse_than_85_percent() {
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness::of(needs(30.0, 40.0, 4.0, 50.0)).arrange(&mut routine, &mut goals);
        // A drink first, then a meal; the bladder is fine, and boredom is
        // not seen to before a trip.
        assert_eq!(raised(&goals), [GoalId::Drink, GoalId::Eat]);
        assert!(goals.priority(GoalId::Eat) > WORKING, "every one of them before the trip");
        assert!(routine.is_getting_ready());
        assert_eq!(
            routine.debug_fields(),
            [("ready to go out", "no".to_string()), ("seeing to", "drink, eat".to_string())]
        );

        // 85% exactly is ready enough.
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        let said = Readiness::of(needs(READY, READY, READY, READY)).arrange(&mut routine, &mut goals);
        assert!(raised(&goals).is_empty());
        assert_eq!(said, [format!("{} is ready to go out for scrap metal", Uid::new(EntityType::Human, 1))]);
    }

    /// Stopped for one need, it tops up every other one past 95% while it is
    /// about it: thirst at 12 is past 15 by the end of the meal, and a
    /// drink after the toilet is a bladder filling on the way out.
    #[test]
    fn once_it_stops_it_tops_every_need_up() {
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness::of(needs(30.0, 12.0, 8.0, 10.0)).arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Drink, GoalId::Eat, GoalId::Relieve]);
        // Drunk: still to top up the rest, though none of them is past 15.
        Readiness::of(needs(32.0, 0.0, 9.0, 10.0)).arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Eat, GoalId::Relieve]);

        // Nothing past 15, nothing to stop for.
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness::of(needs(12.0, 12.0, 12.0, 12.0)).arrange(&mut routine, &mut goals);
        assert_eq!((raised(&goals), routine.state()), (vec![], (true, 0)));
    }

    /// The toilet last: the water of the drink before reaches the bladder
    /// over half an hour.
    #[test]
    fn the_toilet_comes_after_the_drink_and_the_meal() {
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness::of(needs(50.0, 50.0, 50.0, 50.0)).arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Drink, GoalId::Eat, GoalId::Relieve]);
    }

    /// Boredom is not seen to before a trip, however bored: it never takes
    /// a worker off its shift, so it never calls one back from a trip
    /// either, and the go at something was on a TV at home, as often as not
    /// across the district.
    #[test]
    fn a_worker_sets_out_bored_and_is_not_called_away_by_it() {
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        let bored = Readiness::of(needs(0.0, 0.0, 0.0, 100.0));
        bored.arrange(&mut routine, &mut goals);
        assert_eq!((raised(&goals), routine.state()), (vec![], (true, 0)));
        let across = Readiness { cell: ACROSS_THE_SITE, ..Readiness::of(needs(0.0, 0.0, 0.0, 100.0)) };
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        across.arrange(&mut routine, &mut goals);
        assert_eq!((raised(&goals), routine.state()), (vec![], (false, 0)), "not called away by it");
    }

    #[test]
    fn nothing_joins_the_list_while_it_is_seen_to_and_a_fresh_one_is_taken_when_it_is_done() {
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness::of(needs(4.0, 40.0, 0.0, 0.0)).arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Drink]);
        // Hunger creeps past 15 while it goes for its drink: not on the list,
        // so it does not turn it round for the fridge's food.
        Readiness::of(needs(16.0, 40.0, 0.0, 0.0)).arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Drink]);
        // Drunk: a fresh list, with the hunger on it.
        Readiness::of(needs(16.0, 0.0, 0.0, 0.0)).arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Eat]);
        // Eaten: an empty fresh list is ready, and says so once.
        let said = Readiness::of(needs(0.0, 0.0, 0.0, 0.0)).arrange(&mut routine, &mut goals);
        assert_eq!((raised(&goals), said.len()), (vec![], 1));
        assert!(!routine.is_getting_ready());
        // The walk out to the gate works up a thirst: it does not go back.
        let walking_out = Readiness { cell: ACROSS_THE_SITE, ..Readiness::of(needs(5.0, 20.0, 20.0, 20.0)) };
        let said = walking_out.arrange(&mut routine, &mut goals);
        assert_eq!((raised(&goals), said.len()), (vec![], 0));
        assert_eq!(routine.debug_fields()[0], ("ready to go out", "yes".to_string()));
    }

    #[test]
    fn ready_lasts_until_it_has_gone_out_and_it_gets_ready_again_before_it_goes_out_again() {
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness::of(needs(0.0, 0.0, 0.0, 0.0)).arrange(&mut routine, &mut goals);
        assert_eq!(routine.state(), (true, 0));

        // Out there: gone out.
        let thirsty = || needs(0.0, 40.0, 0.0, 0.0);
        let out = Readiness { cell: OUT_THERE, ..Readiness::of(thirsty()) };
        out.arrange(&mut routine, &mut goals);
        assert_eq!((raised(&goals), routine.state()), (vec![], (false, 0)));
        // Back with a packful, thirsty, on its way to the bin: stored first.
        let back = Readiness { cell: ACROSS_THE_SITE, pack: pack(3), ..Readiness::of(thirsty()) };
        back.arrange(&mut routine, &mut goals);
        assert!(raised(&goals).is_empty());
        // Stored: ready for the next one, first.
        Readiness::of(thirsty()).arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Drink]);
    }

    /// A trip starts beside the bin, and that is where getting ready for it
    /// starts: not at home across the district, which is a long walk from
    /// the way out, and not out in the wasteland — hired out there, or loaded
    /// — with nothing to see to anything with. Started, it goes on wherever
    /// the fridge is. Called back in from a trip half done, or called away
    /// by a need anywhere in the district, it gets ready wherever it is — at
    /// the fridge it came in for — before it goes back out, rather than out
    /// and in again for each need in turn.
    #[test]
    fn getting_ready_starts_beside_the_bin_back_in_with_a_trip_half_done_or_called_away() {
        for cell in [ACROSS_THE_SITE, OUT_THERE] {
            let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
            let away = Readiness { cell, ..Readiness::of(needs(40.0, 40.0, 40.0, 40.0)) };
            let said = away.arrange(&mut routine, &mut goals);
            assert_eq!((raised(&goals), said.len(), routine.state()), (vec![], 0, (false, 0)), "{cell:?}");
        }
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness::of(needs(40.0, 0.0, 0.0, 0.0)).arrange(&mut routine, &mut goals);
        let at_the_fridge = Readiness { cell: ACROSS_THE_SITE, ..Readiness::of(needs(40.0, 0.0, 0.0, 0.0)) };
        at_the_fridge.arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Eat]);

        // Called back for a drink with a piece in the pack: everything else
        // too, before it goes back out.
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        let called_back = Readiness { cell: ACROSS_THE_SITE, pack: pack(1), ..Readiness::of(needs(40.0, 80.0, 50.0, 0.0)) };
        called_back.arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Drink, GoalId::Eat, GoalId::Relieve]);
        // Out there with it, nothing.
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        let still_out = Readiness { cell: OUT_THERE, pack: pack(1), ..Readiness::of(needs(40.0, 80.0, 50.0, 0.0)) };
        still_out.arrange(&mut routine, &mut goals);
        assert!(raised(&goals).is_empty());

        // Called away by a need anywhere in the district, nothing in the
        // pack — on the way in from home, say — and ready already or not:
        // everything else too. Short of [`CALLED_AWAY`], not.
        for ready in [false, true] {
            let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
            routine.set_state(ready, 0).unwrap();
            let on_the_way = Readiness { cell: ACROSS_THE_SITE, ..Readiness::of(needs(40.0, 76.0, 50.0, 0.0)) };
            on_the_way.arrange(&mut routine, &mut goals);
            assert_eq!(raised(&goals), [GoalId::Drink, GoalId::Eat, GoalId::Relieve], "ready already: {ready}");
        }
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        let not_yet = Readiness { cell: ACROSS_THE_SITE, ..Readiness::of(needs(40.0, 74.0, 50.0, 0.0)) };
        not_yet.arrange(&mut routine, &mut goals);
        assert!(raised(&goals).is_empty());
    }

    /// No fridge in reach, say: the goal is held off, and the trip does not
    /// wait for it — neither when the list is taken nor half way through it.
    #[test]
    fn a_need_nothing_can_meet_is_not_waited_for() {
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        goals.block(GoalId::Eat);
        Readiness::of(needs(40.0, 0.0, 0.0, 0.0)).arrange(&mut routine, &mut goals);
        assert_eq!((raised(&goals), routine.state()), (vec![], (true, 0)));

        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        let both = Readiness::of(needs(40.0, 40.0, 0.0, 0.0));
        both.arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Drink, GoalId::Eat]);
        goals.block(GoalId::Drink);
        both.arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Eat]);
    }

    #[test]
    fn only_a_collector_on_shift_gets_ready_and_only_for_the_needs_its_body_has() {
        let thirsty = || needs(0.0, 40.0, 0.0, 0.0);
        for (job, clock, why) in [
            (None, on(1, 10.0), "no job"),
            (Some(crate::sim::job::Job::office(Point::new(3, 3))), on(1, 10.0), "an office worker never goes out"),
            (Some(collector()), on(6, 10.0), "Saturday"),
            (Some(collector()), on(1, 17.5), "home time"),
            (Some(collector()), on(1, 8.25), "not set off yet"),
        ] {
            let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
            let said = Readiness { job, clock, ..Readiness::of(thirsty()) }.arrange(&mut routine, &mut goals);
            assert_eq!((raised(&goals), said.len()), (vec![], 0), "{why}");
            assert_eq!(routine.state(), (false, 0), "{why}");
        }
        // Setting off at half past eight: getting ready at home.
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness { clock: on(1, 8.75), ..Readiness::of(thirsty()) }.arrange(&mut routine, &mut goals);
        assert_eq!(raised(&goals), [GoalId::Drink]);

        // Thirst that cannot move is not waited on.
        let mut biology = needs(0.0, 40.0, 0.0, 0.0);
        biology.set_running(ProcessId::Thirst, false);
        let (mut routine, mut goals) = (ReadyRoutine::new(), Goals::new());
        Readiness::of(biology).arrange(&mut routine, &mut goals);
        assert_eq!((raised(&goals), routine.state()), (vec![], (true, 0)));
    }

    /// Over the shift, so the trip waits; under a need past 80, which the
    /// need's own routine wants more anyway.
    #[test]
    fn getting_ready_outranks_the_shift_and_every_need_seen_to_in_turn_does() {
        let last = GETTING_READY - (READY_NEEDS.len() - 1) as f32 * IN_TURN;
        assert!(last > WORKING && last > EXHAUSTED / NEED_PER_PRIORITY, "{last}");
        assert!(GETTING_READY < 81.0 / NEED_PER_PRIORITY);
    }

    #[test]
    fn what_getting_ready_has_still_to_see_to_is_saved_and_nonsense_is_refused() {
        let (mut routine, mut goals) = (super::super::routine::Routine::ready(), Goals::new());
        let map = Map::new(Size::new(4, 4), FLOOR);
        let (occupancy, log, facilities) = (Occupancy::new(map.size()), Log::new(), Facilities::default());
        let think = Think::new(&map, &occupancy, &log, &facilities, 1.0 / 60.0, 0, on(1, 10.0));
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), BESIDE_THE_BIN);
        let (biology, job, pack) = (needs(30.0, 40.0, 0.0, 0.0), collector(), pack(0));
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(&biology),
                memory: &crate::sim::brain::memory::Memory::new(),
                job: Some(&job),
                inventory: Some(&pack),
            },
            &mut goals,
        );
        let saved = routine.save();
        assert_eq!((saved.committed, saved.pending), (false, 0b11));
        let mut loaded = super::super::routine::Routine::ready();
        loaded.restore(&saved).unwrap();
        assert!(loaded.is_getting_ready());
        assert_eq!(loaded.save(), saved);

        for (committed, pending) in [(false, 1 << READY_NEEDS.len()), (true, 0b1)] {
            let nonsense = crate::save::pb::RoutineState { committed, pending, ..saved.clone() };
            assert!(super::super::routine::Routine::ready().restore(&nonsense).is_err(), "{committed} {pending:#b}");
        }
        let work = crate::save::pb::RoutineState { name: "go to work".to_string(), committed: true, pending: 0b1 };
        assert!(super::super::routine::Routine::work().restore(&work).is_err(), "work has no needs to see to");
    }

    #[test]
    fn a_body_whose_tiring_is_switched_off_does_not_sleep_and_lets_go_of_a_commitment() {
        let mut biology = tired(90.0);
        let mut routine = SleepRoutine::new();
        assert!(sleep_priority(&mut routine, &biology, night()) > BUSY);

        biology.set_running(ProcessId::Energy, false);
        assert_eq!(sleep_priority(&mut routine, &biology, night()), 0.0);
        assert_eq!(routine.debug_fields(), [("sleepy", "no".to_string())]);
    }
}
