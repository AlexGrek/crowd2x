//! [`FollowGoal`]: a pet dog staying with its owner.
//!
//! Always close, never on a lead. **Within [`ROAM`] cells of its owner a dog
//! roams freely**: a spot of its own picked there, walked to, perhaps a
//! pause, another. **Past it, it comes**: to a spot within [`CLOSE`] cells of
//! the owner, wherever the owner has got to. A hungry dog does not roam
//! ([`pets::HUNGRY`](crate::sim::pets::HUNGRY)): it keeps within [`CLOSE`] and
//! waits to be fed.
//!
//! The owner is not a feature of the map, it moves, and a dog may not read
//! it: where it stands is its [`Bond`](crate::sim::pets::Bond), found in the
//! crowd's [`Bonds`](crate::sim::pets::Bonds) by a binary search. A walk
//! aimed at a spot the owner has since left behind is cut short in
//! [`keep_up`](GoalExecutor::keep_up) and aimed again.
//!
//! **An owner that is not in the world** — dead, or gone — is one the dog
//! forgets: it keeps no company and is a stray, wandering as one.

use rand::rngs::SmallRng;
use rand::RngExt;

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};
use crate::sim::pets::HUNGRY;
use crate::sim::rng::tick_rng;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{PATIENCE, WAIT_FOR_A_GAP};

/// How far from its owner a dog roams freely, in cells either way.
pub const ROAM: i32 = 4;

/// How near a dog comes when it has strayed too far, and stays when it is
/// hungry.
pub const CLOSE: i32 = 2;

/// Cells tried for a spot before giving up this tick. A miss is cheap — no
/// search ran — and tried again next tick.
pub const TRIES: u32 = 8;

/// The longest a dog stands still after reaching a spot, in watched seconds.
pub const PAUSE: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FollowGoal {
    /// The spot it is walking to.
    target: Option<Point>,
    /// Walks to it cut short by somebody in the way, in a row.
    retries: u8,
}

impl FollowGoal {
    pub fn new() -> FollowGoal {
        FollowGoal::default()
    }

    pub fn target(&self) -> Option<Point> {
        self.target
    }

    /// A cell within `reach` of the owner, a different one from where the dog
    /// is and the owner is, that it can stand in and get to - and that is not a
    /// doorway, where a dog standing a while is an owner who cannot get by.
    fn pick(ctx: &GoalCtx<'_>, owner: Point, reach: i32, rng: &mut SmallRng) -> Option<Point> {
        let here = ctx.body.center_position();
        let navigation = ctx.think.map.navigation();
        for _ in 0..TRIES {
            let spot = owner.offset(rng.random_range(-reach..=reach), rng.random_range(-reach..=reach));
            if spot != here
                && spot != owner
                && ctx.think.is_passable(spot)
                && !ctx.think.doors.is_door(spot)
                && !ctx.think.occupancy.is_occupied(spot)
                && navigation.may_reach(here, spot)
                && ctx.think.may_use(spot, ctx.body.home())
            {
                return Some(spot);
            }
        }
        None
    }

    fn forget(&mut self) {
        *self = FollowGoal::default();
    }
}

/// How far from its owner a dog in this state may be, in cells.
fn reach(hunger: f32) -> i32 {
    if hunger >= HUNGRY { CLOSE } else { ROAM }
}

impl GoalExecutor for FollowGoal {
    fn goal(&self) -> GoalId {
        GoalId::Follow
    }

    fn save(&self) -> pb::Executor {
        let FollowGoal { target, retries } = *self;
        pb::Executor::Wander(pb::Wander { target: target.map(Into::into), retries: retries as u32 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Wander(follow) = saved else {
            return invalid("following is a target and its retries");
        };
        let Ok(retries) = u8::try_from(follow.retries) else {
            return invalid(format!("follow: {} retries", follow.retries));
        };
        self.target = follow.target.map(Into::into);
        self.retries = retries;
        Ok(())
    }

    /// The owner has moved on while the dog was doing something else: where
    /// it was going is not where it should be.
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.forget();
    }

    /// Every tick a task is going on: has the spot it is walking to been left
    /// behind by the owner? Or, standing a while, has the owner walked off?
    /// Then pick a new spot, near it.
    fn keep_up(&mut self, ctx: &mut GoalCtx<'_>, current: &Task) -> bool {
        let me = ctx.body.uid();
        let (Some(mine), Some(owner)) = (ctx.think.bonds.get(me), ctx.think.bonds.partner(me)) else {
            return false;
        };
        let from = match current {
            Task::MoveTo(walk) => walk.cell,
            Task::Wait(_) => ctx.body.center_position(),
            _ => return false,
        };
        from.chebyshev_distance(owner.cell) > reach(mine.hunger) + 1
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let me = ctx.body.uid();
        let Some(mine) = ctx.think.bonds.get(me).copied() else {
            return GoalProgress::Blocked;
        };
        let Some(owner) = ctx.think.bonds.get(mine.with).copied() else {
            // Not in the world any more: dead, or gone.
            ctx.memory.set_companion(None);
            return GoalProgress::Blocked;
        };

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                if ctx.blocked_by.is_some() && self.retries < PATIENCE {
                    // A body in the way: bodies move.
                    self.retries += 1;
                    let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                    return GoalProgress::Working;
                }
                // No way there: not cheap to find out.
                self.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => {
                self.target = None;
                self.retries = 0;
                // Reached a spot: now and then, stand a while.
                let mut rng = tick_rng(me, ctx.think.tick);
                if rng.random_range(0..2) == 0 {
                    let _ = ctx.tasks.push_back(Task::wait(rng.random_range(0.5..PAUSE)));
                    return GoalProgress::Working;
                }
            }
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        let reach = reach(mine.hunger);
        let here = ctx.body.center_position();
        // Too far: come to the owner. Near enough: roam round it.
        let within = if here.chebyshev_distance(owner.cell) > reach { CLOSE } else { reach };
        let mut rng = tick_rng(me, ctx.think.tick);
        match Self::pick(ctx, owner.cell, within, &mut rng) {
            Some(spot) => {
                self.target = Some(spot);
                let _ = ctx.tasks.push_back(Task::move_to(spot));
            }
            None => self.target = None,
        }
        GoalProgress::Working
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "following to",
            self.target.map_or_else(|| "nowhere".to_string(), |spot| format!("{}, {}", spot.x, spot.y)),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::pets::rig::{dog, owner_stands};
    use crate::sim::testing::World;
    use crate::sim::GameEntity;
    use std::collections::BTreeSet;

    const OWNER: Point = Point::new(20, 20);

    fn field() -> World {
        World::new(Map::new(Size::new(41, 41), FLOOR))
    }

    #[test]
    fn a_pet_roams_freely_within_range_of_its_owner_and_never_beyond() {
        let mut world = field();
        owner_stands(&mut world, OWNER);
        let mut dog = dog(Point::new(20, 22), 0.0);
        let (mut seen, mut furthest) = (BTreeSet::new(), 0);
        for _ in 0..4000 {
            world.step(&mut dog);
            assert_eq!(dog.brain().top_goal(), GoalId::Follow);
            let here = dog.center_position();
            furthest = furthest.max(here.chebyshev_distance(OWNER));
            seen.insert(here);
        }
        assert!(furthest <= ROAM, "strayed {furthest} cells");
        assert!(furthest >= ROAM - 1, "never roamed further than {furthest}");
        assert!(seen.len() >= 20, "it only visited {} cells", seen.len());
    }

    #[test]
    fn a_pet_far_from_its_owner_comes_to_it() {
        let mut world = field();
        owner_stands(&mut world, OWNER);
        let mut dog = dog(Point::new(2, 2), 0.0);
        for _ in 0..800 {
            world.step(&mut dog);
        }
        let near = dog.center_position().chebyshev_distance(OWNER);
        assert!(near <= ROAM, "still {near} cells away");
    }

    /// The owner walks off at its own pace and the dog keeps up, aiming
    /// again every time the spot it was heading for is left behind.
    #[test]
    fn a_pet_keeps_up_with_an_owner_who_walks_away() {
        let mut world = field();
        let mut dog = dog(Point::new(5, 20), 0.0);
        let mut furthest = 0;
        for tick in 0..1200 {
            let owner = Point::new((5 + (tick as f32 * 4.0 / 64.0) as i32).min(35), 20);
            owner_stands(&mut world, owner);
            world.step(&mut dog);
            if tick > 60 {
                furthest = furthest.max(dog.center_position().chebyshev_distance(owner));
            }
        }
        assert!(furthest <= 2 * ROAM + 1, "fell {furthest} cells behind");
        assert!(dog.center_position().chebyshev_distance(Point::new(35, 20)) <= ROAM);
    }

    /// A hungry dog is not off exploring: it waits at its owner's heel to be
    /// fed.
    #[test]
    fn a_hungry_pet_keeps_close_to_its_owner() {
        let mut world = field();
        owner_stands(&mut world, OWNER);
        let mut dog = dog(Point::new(20, 22), 80.0);
        let mut furthest = 0;
        for _ in 0..2000 {
            world.step(&mut dog);
            furthest = furthest.max(dog.center_position().chebyshev_distance(OWNER));
        }
        assert!(furthest <= CLOSE, "wandered {furthest} cells off hungry");
        assert!(furthest >= 1, "it did not even move");
    }

    /// An owner that is not in the world any more - dead, or gone - is one
    /// the dog forgets, and it wanders as a stray does.
    #[test]
    fn a_pet_forgets_an_owner_who_is_gone_and_wanders() {
        let mut world = field();
        let mut dog = dog(Point::new(20, 22), 0.0);
        assert!(dog.companion().is_some());
        for _ in 0..200 {
            world.step(&mut dog);
        }
        assert_eq!(dog.companion(), None);
        assert_eq!(dog.brain().top_goal(), GoalId::Wander);
    }

    /// A dog stood in a doorway for the length of its pause is an owner who
    /// cannot get by: it crosses one when it must, and never stops in it.
    #[test]
    fn a_pet_never_stops_in_a_doorway() {
        let mut map = Map::new(Size::new(41, 41), FLOOR);
        for y in 0..41 {
            map.set_terrain(Point::new(21, y), crate::map::WALL);
        }
        let door = Point::new(21, 20);
        map.set_terrain(door, FLOOR);
        crate::sim::testing::prop_at(&mut map, crate::sim::door::DOOR, door);
        let mut world = World::new(map);
        owner_stands(&mut world, Point::new(20, 20));
        // Hungry, so its range is the five by five round its owner, the
        // doorway one of the twenty-four cells in it.
        let mut dog = dog(Point::new(19, 20), 80.0);
        let (mut in_door, mut longest, mut crossed) = (0, 0, false);
        for _ in 0..20000 {
            world.step(&mut dog);
            if dog.center_position() == door {
                in_door += 1;
                longest = longest.max(in_door);
            } else {
                in_door = 0;
            }
            crossed |= dog.center_position().x > 21;
        }
        assert!(crossed, "it never went through the door");
        assert!(longest < 20, "stood {longest} ticks in a doorway");
    }

    #[test]
    fn what_it_follows_to_is_saved_and_a_broken_save_refused() {
        let goal = FollowGoal { target: Some(Point::new(3, -4)), retries: 2 };
        let mut back = FollowGoal::new();
        back.restore(&goal.save()).unwrap();
        assert_eq!(back, goal);
        assert!(back.restore(&pb::Executor::Wander(pb::Wander { target: None, retries: 300 })).is_err());
        assert!(back.restore(&pb::Executor::Seek(pb::Seek::default())).is_err());
    }
}
