//! The routines that exist.

use super::goal::{GoalId, Goals};
use super::routine::{Routine, RoutineCtx};

/// Hunger at which a human decides it is time to eat.
pub const PECKISH: f32 = 60.0;

/// Hunger a human has to be brought down to before it stops thinking about
/// food. Well below [`PECKISH`], which is the point — see [`KeepFedRoutine`].
pub const SATED: f32 = 25.0;

/// Hunger per point of priority. Hunger is 0-100 and the floor under
/// everything ([`StayBusyRoutine`]) is 0.1, so a human who has only just
/// committed to eating (hunger 25-ish) already outranks wandering five times
/// over, and a starving one is at 2.
const HUNGER_PER_PRIORITY: f32 = 50.0;

/// Maps hunger onto how badly [`GoalId::Eat`] is wanted.
///
/// **With hysteresis.** It commits at [`PECKISH`] and stays committed until
/// hunger is down to [`SATED`], so a stat hovering at a single threshold does
/// not make a human walk half way to the fridge and turn round — and a meal
/// that only takes the edge off sends it back for a second one rather than
/// leaving it one step from peckish.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KeepFedRoutine {
    committed: bool,
}

impl KeepFedRoutine {
    pub fn new() -> KeepFedRoutine {
        KeepFedRoutine::default()
    }
}

impl Routine for KeepFedRoutine {
    fn name(&self) -> &'static str {
        "keep fed"
    }

    fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        // Nothing to keep fed.
        let Some(stats) = ctx.stats else {
            self.committed = false;
            return;
        };
        let hunger = stats.hunger();
        if hunger >= PECKISH {
            self.committed = true;
        } else if hunger <= SATED {
            self.committed = false;
        }
        if self.committed {
            goals.raise_to(GoalId::Eat, hunger / HUNGER_PER_PRIORITY);
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("hungry", if self.committed { "yes" } else { "no" }.to_string())]
    }
}

/// How much [`StayBusyRoutine`] wants a unit wandering.
pub const BUSY: f32 = 0.1;

/// The floor the list rests on: wander, a little, always. A unit with no
/// pressing need is not standing still.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StayBusyRoutine;

impl Routine for StayBusyRoutine {
    fn name(&self) -> &'static str {
        "stay busy"
    }

    fn arrange(&mut self, _ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        goals.raise_to(GoalId::Wander, BUSY);
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
    use crate::sim::stats::Stats;
    use crate::sim::uid::{EntityType, Uid};

    fn eat_priority(routine: &mut KeepFedRoutine, hunger: f32) -> f32 {
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
        let stats = Stats::calm().with_hunger(hunger);
        let mut goals = Goals::new();
        routine.arrange(
            &RoutineCtx {
                think: &think,
                body: &body,
                stats: Some(&stats),
            },
            &mut goals,
        );
        goals.priority(GoalId::Eat)
    }

    #[test]
    fn a_fed_human_does_not_want_to_eat() {
        assert_eq!(eat_priority(&mut KeepFedRoutine::new(), 10.0), 0.0);
    }

    #[test]
    fn a_hungry_human_wants_to_eat_more_than_it_wants_to_wander() {
        assert!(eat_priority(&mut KeepFedRoutine::new(), 70.0) > BUSY);
    }

    #[test]
    fn hunger_hovering_at_the_threshold_does_not_flip_the_decision() {
        let mut routine = KeepFedRoutine::new();
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
}
