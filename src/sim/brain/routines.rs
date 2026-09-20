//! The routines that exist.

use crate::sim::biology::{ProcessId, Stats};

use super::goal::{GoalId, Goals};
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
            goals.raise_to(self.need.goal, level / NEED_PER_PRIORITY);
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(self.need.feeling, if self.committed { "yes" } else { "no" }.to_string())]
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
    use crate::sim::uid::{EntityType, Uid};

    /// The priority `routine` gives its goal for a human whose `stats` these are.
    fn priority(routine: &mut NeedRoutine, stats: Stats) -> f32 {
        priority_in(routine, &Biology::new(stats))
    }

    fn priority_in(routine: &mut NeedRoutine, biology: &Biology) -> f32 {
        let map = Map::new(Size::new(2, 2), FLOOR);
        let (occupancy, log, features) = (Occupancy::new(map.size()), Log::new(), Features::default());
        let think = Think {
            map: &map,
            occupancy: &occupancy,
            log: &log,
            features: &features,
            dt: 1.0 / 60.0,
            tick: 0,
        };
        let body = Body::at_cell(Uid::new(EntityType::Human, 1), Point::new(0, 0));
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                biology: Some(biology),
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
}
