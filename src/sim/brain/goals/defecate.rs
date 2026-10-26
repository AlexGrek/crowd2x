//! [`DefecateGoal`]: a pet dog relieving itself, on the street.
//!
//! Wanted once its bowel is full ([`pets::MUST_GO`](crate::sim::pets::MUST_GO):
//! the meal it was given has passed through). **Only out of doors**: on ground
//! with no ceiling over it and nobody's property — a street, a yard, a park. A
//! dog on such ground squats where it stands; one indoors picks a spot
//! within [`RADIUS`] cells of where it is, walks there and squats, and one that
//! can find none (its owner is deep in a building) is held off and goes on
//! keeping company until it can ([`GoalProgress::Blocked`]).
//!
//! The mess is the world's ([`Effect::Defecate`](crate::sim::Effect), granted
//! in the world step), and so is the dog's relief.

use rand::RngExt;

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};
use crate::sim::rng::tick_rng;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::DEFECATE_SECONDS;
use super::{PATIENCE, WAIT_FOR_A_GAP};

/// How far from where it is a dog looks for a spot, in cells either way.
pub const RADIUS: i32 = 6;

/// Cells tried for a spot before giving up this tick.
pub const TRIES: u32 = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DefecateGoal {
    /// The spot it has chosen.
    target: Option<Point>,
    /// Walks to it cut short by somebody in the way, in a row.
    retries: u8,
}

impl DefecateGoal {
    pub fn new() -> DefecateGoal {
        DefecateGoal::default()
    }

    /// Whether a dog may foul `cell`: out of doors on public ground.
    fn fit(ctx: &GoalCtx<'_>, cell: Point) -> bool {
        ctx.think.is_passable(cell)
            && !ctx.think.doors.is_door(cell)
            && !ctx.think.map.has_ceiling(cell)
            && ctx.think.properties.of(cell).is_none()
            && ctx.think.dirt.puddle(cell).is_none()
    }

    /// Where to go: right here if it will do, otherwise a spot that will.
    fn spot(ctx: &GoalCtx<'_>) -> Option<Point> {
        let here = ctx.body.center_position();
        if Self::fit(ctx, here) {
            return Some(here);
        }
        let navigation = ctx.think.map.navigation();
        let mut rng = tick_rng(ctx.body.uid(), ctx.think.tick);
        for _ in 0..TRIES {
            let spot = here.offset(rng.random_range(-RADIUS..=RADIUS), rng.random_range(-RADIUS..=RADIUS));
            if spot != here
                && Self::fit(ctx, spot)
                && !ctx.think.occupancy.is_occupied(spot)
                && navigation.may_reach(here, spot)
            {
                return Some(spot);
            }
        }
        None
    }

    fn forget(&mut self) {
        *self = DefecateGoal::default();
    }
}

impl GoalExecutor for DefecateGoal {
    fn goal(&self) -> GoalId {
        GoalId::Defecate
    }

    fn save(&self) -> pb::Executor {
        let DefecateGoal { target, retries } = *self;
        pb::Executor::Wander(pb::Wander { target: target.map(Into::into), retries: retries as u32 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Wander(goal) = saved else {
            return invalid("defecating is a target and its retries");
        };
        let Ok(retries) = u8::try_from(goal.retries) else {
            return invalid(format!("defecate: {} retries", goal.retries));
        };
        self.target = goal.target.map(Into::into);
        self.retries = retries;
        Ok(())
    }

    /// A spot chosen is not stale, but a walk half done is: queue it again
    /// from where it is now.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        self.retries = 0;
        if let Some(spot) = self.target {
            if ctx.body.center_position() != spot {
                let _ = ctx.tasks.push_back(Task::move_to(spot));
            }
            let _ = ctx.tasks.push_back(Task::defecate(spot, DEFECATE_SECONDS));
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        match (last, ctx.finished) {
            (TaskResult::Success, Some(Task::Defecate(_))) => {
                self.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                if ctx.blocked_by.is_some() && self.retries < PATIENCE {
                    self.retries += 1;
                    let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                    if let Some(spot) = self.target {
                        let _ = ctx.tasks.push_back(Task::move_to(spot));
                        let _ = ctx.tasks.push_back(Task::defecate(spot, DEFECATE_SECONDS));
                    }
                    return GoalProgress::Working;
                }
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // Not where it meant to be: work it out again.
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        let Some(spot) = Self::spot(ctx) else {
            self.forget();
            return GoalProgress::Blocked;
        };
        self.target = Some(spot);
        if ctx.body.center_position() != spot {
            let _ = ctx.tasks.push_back(Task::move_to(spot));
        }
        let _ = ctx.tasks.push_back(Task::defecate(spot, DEFECATE_SECONDS));
        GoalProgress::Working
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "relieving at",
            self.target.map_or_else(|| "nowhere".to_string(), |spot| format!("{}, {}", spot.x, spot.y)),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::dirt::Dirt;
    use crate::sim::pets::rig::{dog_that_must_go, owner_stands};
    use crate::sim::testing::World;
    use crate::sim::GameEntity;

    /// Every cell with a stain on it.
    fn stains(world: &World, map: &Map) -> Vec<Point> {
        map.size().points().filter(|&cell| world.dirt.at(cell) != Dirt::CLEAN).collect()
    }

    #[test]
    fn a_dog_with_a_full_bowel_fouls_the_street_where_it_stands() {
        let map = Map::new(Size::new(15, 15), FLOOR);
        let mut world = World::new(map.clone());
        let here = Point::new(7, 7);
        owner_stands(&mut world, here);
        let mut dog = dog_that_must_go(here);
        for _ in 0..600 {
            world.step(&mut dog);
            if world.log_contains("fouled the street") {
                break;
            }
        }
        assert!(world.log_contains("fouled the street"), "{:?}", dog.brain().goals().ranked());
        let mess = stains(&world, &map);
        assert!(!mess.is_empty());
        assert!(
            mess.iter().any(|&cell| world.dirt.at(cell).overall() > crate::sim::brain::goals::clean::DIRTY),
            "left nothing a cleaner would take notice of: {mess:?}"
        );
        assert_eq!(dog.gut().bowel(), 0.0, "relieved");
    }

    /// Indoors is nobody's lavatory: it goes out to open ground and fouls
    /// that.
    #[test]
    fn a_dog_indoors_goes_out_to_foul_open_ground() {
        let mut map = Map::new(Size::new(17, 9), FLOOR);
        for x in 0..8 {
            for y in 0..9 {
                map.set_ceiling(Point::new(x, y), true);
            }
        }
        let mut world = World::new(map.clone());
        owner_stands(&mut world, Point::new(3, 5));
        let mut dog = dog_that_must_go(Point::new(3, 4));
        for _ in 0..3000 {
            world.step(&mut dog);
            if world.log_contains("fouled the street") {
                break;
            }
        }
        let mess = stains(&world, &map);
        assert!(world.log_contains("fouled the street"), "{mess:?}");
        assert!(!mess.is_empty());
        for cell in mess {
            assert!(!map.has_ceiling(cell), "fouled the floor of a room at {cell:?}");
        }
    }

    /// With no open ground in reach it is held off and goes on keeping its
    /// owner company, rather than standing there with its legs crossed.
    #[test]
    fn a_dog_with_no_open_ground_to_go_to_is_held_off_and_keeps_company() {
        let mut map = Map::new(Size::new(15, 15), FLOOR);
        for x in 0..15 {
            for y in 0..15 {
                map.set_ceiling(Point::new(x, y), true);
            }
        }
        let mut world = World::new(map.clone());
        let owner = Point::new(7, 7);
        owner_stands(&mut world, owner);
        let mut dog = dog_that_must_go(owner.offset(0, 1));
        let mut held_off = false;
        let mut furthest = 0;
        for _ in 0..3000 {
            world.step(&mut dog);
            held_off |= dog.brain().goals().cooldown(GoalId::Defecate) > 0;
            furthest = furthest.max(dog.center_position().chebyshev_distance(owner));
        }
        assert!(held_off);
        assert!(stains(&world, &map).is_empty(), "fouled indoors");
        assert!(dog.gut().must_go(), "still full");
        assert!(furthest > 0 && furthest <= crate::sim::brain::goals::follow::ROAM, "{furthest}");
    }

    #[test]
    fn where_it_means_to_go_is_saved_and_a_broken_save_refused() {
        let goal = DefecateGoal { target: Some(Point::new(-2, 9)), retries: 1 };
        let mut back = DefecateGoal::new();
        back.restore(&goal.save()).unwrap();
        assert_eq!(back, goal);
        assert!(back.restore(&pb::Executor::Wander(pb::Wander { target: None, retries: 256 })).is_err());
        assert!(back.restore(&pb::Executor::Seek(pb::Seek::default())).is_err());
    }
}
