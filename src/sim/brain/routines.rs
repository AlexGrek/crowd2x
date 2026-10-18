//! The routines that exist.

use crate::sim::biology::{ProcessId, Stats};
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
            goals.raise_to(self.need.goal, priority);
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(self.need.feeling, if self.committed { "yes" } else { "no" }.to_string())]
    }
}

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

/// Go to work: want the job's goal ([`JobKind::goal`](crate::sim::job::JobKind::goal)
/// — [`GoalId::Work`] at a desk, [`GoalId::Collect`] for a scrap collector)
/// at [`WORKING`] for as long as the shift is due
/// ([`Job::is_due`](crate::sim::job::Job::is_due) — its hours on its
/// days, and the commute before them).
///
/// It reads the clock and the job and no stat, so it needs no hysteresis:
/// nothing the goal does moves when the shift is. What it remembers is only
/// what it decided last tick, for the brains menu and a save.
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
        }
        // An undertaker with a body in hand sees it to the fire, shift over
        // or not: it is not taken home, and nothing else may be done with
        // the hand full of it.
        if let Some(job) = ctx.job
            && job.kind == JobKind::Undertaker
            && ctx.carrying == Some(crate::sim::item::ItemKind::Body)
        {
            goals.raise_to(GoalId::Bury, WORKING);
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("on shift", if self.on_shift { "yes" } else { "no" }.to_string())]
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
    use crate::sim::feature::Features;
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
        let (occupancy, log, features, fridges) = (
            Occupancy::new(map.size()),
            Log::new(),
            Features::default(),
            crate::sim::fridge::Fridges::default(),
        );
        let think = Think {
            map: &map,
            occupancy: &occupancy,
            log: &log,
            features: &features,
            fridges: &fridges,
            doors: &Default::default(),
            properties: &Default::default(),
            corpses: &crate::sim::corpses::NONE,
            dt: 1.0 / 60.0,
            tick: 0,
            clock: crate::sim::clock::Clock::after_watching(0.0),
        };
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(biology),
                memory: &crate::sim::brain::memory::Memory::new(),
                job: None,
                carrying: None,
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
        let (occupancy, log, features, fridges) = (
            Occupancy::new(map.size()),
            Log::new(),
            Features::default(),
            crate::sim::fridge::Fridges::default(),
        );
        let think = Think {
            map: &map,
            occupancy: &occupancy,
            log: &log,
            features: &features,
            fridges: &fridges,
            doors: &Default::default(),
            properties: &Default::default(),
            corpses: &crate::sim::corpses::NONE,
            dt: 1.0 / 60.0,
            tick: 0,
            clock,
        };
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(biology),
                memory: &crate::sim::brain::memory::Memory::new(),
                job,
                carrying: None,
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
        let (occupancy, log, features, fridges) = (
            Occupancy::new(map.size()),
            Log::new(),
            Features::default(),
            crate::sim::fridge::Fridges::default(),
        );
        let think = Think {
            map: &map,
            occupancy: &occupancy,
            log: &log,
            features: &features,
            fridges: &fridges,
            doors: &Default::default(),
            properties: &Default::default(),
            corpses: &crate::sim::corpses::NONE,
            dt: 1.0 / 60.0,
            tick: 0,
            clock,
        };
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(&Biology::new(Stats::calm())),
                memory: &crate::sim::brain::memory::Memory::new(),
                job,
                carrying: None,
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

    /// A scrap collector's shift is the collect goal, at the same priority
    /// and hours, and never the office's.
    #[test]
    fn a_scrap_collector_on_shift_wants_to_collect_rather_than_to_work() {
        let job = crate::sim::job::Job::collector(Point::new(3, 3));
        let wanted = |clock: Clock| {
            let map = Map::new(Size::new(2, 2), FLOOR);
            let (occupancy, log, features, fridges) = (
                Occupancy::new(map.size()),
                Log::new(),
                Features::default(),
                crate::sim::fridge::Fridges::default(),
            );
            let think = Think {
                map: &map,
                occupancy: &occupancy,
                log: &log,
                features: &features,
                fridges: &fridges,
                doors: &Default::default(),
                properties: &Default::default(),
                corpses: &crate::sim::corpses::NONE,
                dt: 1.0 / 60.0,
                tick: 0,
                clock,
            };
            let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
            let mut goals = Goals::new();
            WorkRoutine::new().arrange(
                &RoutineCtx {
                    think: &think,
                    body: &body,
                    biology: Some(&Biology::new(Stats::calm())),
                    memory: &crate::sim::brain::memory::Memory::new(),
                    job: Some(&job),
                    carrying: None,
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
