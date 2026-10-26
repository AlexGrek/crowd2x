//! [`Bite`]: a deathdog bites whoever is within reach.
//!
//! The end of a [`Chase`](super::Chase) (`HuntGoal`). Instant: whoever stands
//! in the cell the quarry was found in — or failing that, in one of the eight
//! round the deathdog, in a fixed order — is bitten, if it is a person or dog
//! outside the site; nobody inside is ever bitten. How hard comes from the
//! deathdog's own dice for the tick ([`BITE_DAMAGE`]), and the world is asked
//! to deal it ([`Effect::Bite`]): the wound is the bitten one's to feel.

use rand::RngExt;

use crate::map::Point;
use crate::sim::deathdogs::BITE_DAMAGE;
use crate::sim::rng::tick_rng;
use crate::sim::uid::EntityType;
use crate::sim::Effect;

use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// The eight cells round a deathdog, in the order they are bitten into.
const AROUND: [(i32, i32); 8] = [(0, 1), (1, 0), (0, -1), (-1, 0), (1, 1), (1, -1), (-1, -1), (-1, 1)];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bite {
    /// Where the quarry was last found.
    pub at: Point,
    /// Always 0: a bite is over as soon as it lands. Kept so it saves as
    /// every timed task does.
    pub seconds: f32,
}

impl TaskExecutor for Bite {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let here = ctx.here();
        let site = ctx.think.map.site();
        let occupancy = ctx.think.occupancy;
        let prey = |cell: Point| {
            cell != here
                && here.chebyshev_distance(cell) <= 1
                && !site.contains(cell)
                && occupancy.occupant(cell).is_some_and(|uid| uid.kind().is_some_and(EntityType::is_deathdog_prey))
        };
        let Some(cell) = std::iter::once(self.at).chain(AROUND.map(|(dx, dy)| here.offset(dx, dy))).find(|&cell| prey(cell)) else {
            return TaskResult::Failed;
        };
        let damage = tick_rng(ctx.walk.body().uid(), ctx.think.tick).random_range(BITE_DAMAGE);
        *ctx.effect = Effect::Bite { at: cell, damage };
        TaskResult::Success
    }

    fn describe(&self) -> String {
        format!("bite at {}, {}", self.at.x, self.at.y)
    }
}

impl Task {
    /// Bite whoever is within reach, the one at `at` first.
    pub const fn bite(at: Point) -> Task {
        Task::Bite(Bite { at, seconds: 0.0 })
    }
}

/// Saved as `bite`: the cell, and no time.
impl TimedTask for Bite {
    const NAME: &'static str = "bite";

    fn new(cell: Point, seconds: f32) -> Bite {
        Bite { at: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.at
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }
}

#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::uid::Uid;

    fn rig() -> Rig {
        let mut map = Map::new(Size::new(2, 2), FLOOR).with_wasteland(5);
        map.load_chunks(map.navigation_area());
        Rig::new(map, Point::new(-5, -5))
    }

    #[test]
    fn a_person_in_reach_outside_is_bitten_for_twenty_to_sixty() {
        let mut rig = rig();
        let victim = Uid::new(EntityType::Human, 3);
        let _ = rig.world.occupancy.claim(Point::new(-4, -4), victim);
        let mut task = Task::bite(Point::new(-4, -5));
        assert_eq!(rig.tick(&mut task), TaskResult::Success, "a neighbour, the quarry having stepped aside");
        let Effect::Bite { at, damage } = rig.effect else { panic!("{:?}", rig.effect) };
        assert_eq!(at, Point::new(-4, -4));
        assert!(BITE_DAMAGE.contains(&damage), "{damage}");
    }

    #[test]
    fn nobody_in_reach_another_deathdog_or_somebody_on_the_site_is_not_bitten() {
        let mut rig = rig();
        assert_eq!(rig.tick(&mut Task::bite(Point::new(-4, -5))), TaskResult::Failed, "nobody");
        let _ = rig.world.occupancy.claim(Point::new(-4, -5), Uid::new(EntityType::Deathdog, 4));
        assert_eq!(rig.tick(&mut Task::bite(Point::new(-4, -5))), TaskResult::Failed, "another deathdog");

        let mut rig = Rig::new(rig.world.map.clone(), Point::new(-1, 0));
        let _ = rig.world.occupancy.claim(Point::new(0, 0), Uid::new(EntityType::Human, 5));
        assert_eq!(rig.tick(&mut Task::bite(Point::new(0, 0))), TaskResult::Failed, "inside the site");
        assert_eq!(rig.effect, Effect::None);
    }
}
