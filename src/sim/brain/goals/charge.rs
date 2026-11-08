//! A robot's charging: to the nearest charging dock with a free side, and a
//! stretch of charging there at a time (`Task::charge`) — `Achieved` after
//! each, and planned again on the spot while `RobotRoutine` still wants it,
//! which it does until the battery is full (`sim::robots`).
//!
//! **A dock with a robot at it is not given up on.** Every dock taken, it goes
//! to the nearest anyway and, once near, waits a while and looks again — a
//! robot in want of charge has nowhere better to be. A body in the way of the
//! walk is the same: a short wait, and the nearest free dock again. Only a
//! dock there is no way to is remembered out of reach (`Seek::setback`).

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::CHARGE_SECONDS;
use super::{stand_beside, Seek, Stand};

/// How long a robot waits near a station whose docks are all taken before it
/// looks again: two world minutes.
pub const QUEUE_SECONDS: f32 = watched(2.0 * MINUTE);

/// How near the nearest dock a robot waits, when every dock is taken, rather
/// than walking closer.
const NEAR: i32 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ChargeGoal {
    seek: Seek,
}

impl ChargeGoal {
    pub fn new() -> ChargeGoal {
        ChargeGoal::default()
    }

    /// Whether a robot could stand beside the dock in `dock` now: a side of
    /// it that is open and nobody else's.
    fn has_room(ctx: &GoalCtx<'_>, dock: Point) -> bool {
        let uid = ctx.body.uid();
        let key = ctx.body.home();
        dock.cardinal_neighbours()
            .into_iter()
            .any(|side| ctx.think.is_passable_with(side, key) && ctx.think.occupancy.is_free_for(side, uid))
    }

    /// The nearest powered dock it may use, not remembered out of reach, and
    /// — `free` — with room beside it.
    fn nearest(ctx: &GoalCtx<'_>, free: bool) -> Option<Point> {
        let here = ctx.body.center_position();
        let now = ctx.think.clock.elapsed();
        let places = ctx.memory.out_of_reach();
        let key = ctx.body.home();
        ctx.think.features.nearest_where(FeatureKind::Charger, here, |cell| {
            ctx.think.may_use(cell, key) && !places.contains(cell, now) && (!free || Self::has_room(ctx, cell))
        })
    }

    /// Walk beside the dock it chose (unless there already) and charge.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(dock) = self.seek.target else {
            return false;
        };
        match stand_beside(ctx, dock) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::charge(dock, CHARGE_SECONDS));
        true
    }

    /// A free dock, or near the nearest taken one a wait, or the walk to it.
    fn choose(&mut self, ctx: &mut GoalCtx<'_>) -> GoalProgress {
        if let Some(dock) = Self::nearest(ctx, true) {
            self.seek.choose(dock);
            return if self.plan(ctx) { GoalProgress::Working } else { GoalProgress::Blocked };
        }
        let Some(dock) = Self::nearest(ctx, false) else {
            return GoalProgress::Blocked;
        };
        if ctx.body.center_position().chebyshev_distance(dock) <= NEAR {
            self.seek.forget();
            let _ = ctx.tasks.push_back(Task::wait(QUEUE_SECONDS));
            return GoalProgress::Working;
        }
        self.seek.choose(dock);
        let _ = ctx.tasks.push_back(Task::move_beside(dock));
        GoalProgress::Working
    }
}

impl GoalExecutor for ChargeGoal {
    fn goal(&self) -> GoalId {
        GoalId::Charge
    }

    fn save(&self) -> pb::Executor {
        pb::Executor::Seek(self.seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// From what is true now: the dock it chose, if it still has room.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        self.seek.steady();
        if self.seek.target.is_some_and(|dock| !Self::has_room(ctx, dock)) {
            self.seek.forget();
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.inventory.is_none() {
            return GoalProgress::Blocked;
        }
        match (last, ctx.finished) {
            (TaskResult::Success, Some(Task::Charge(_))) => return GoalProgress::Achieved,
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // Somebody in the way, or at the dock: a short wait, and the
                // nearest free dock again.
                if ctx.blocked_by.is_some() {
                    self.seek.forget();
                    let _ = ctx.tasks.push_back(Task::wait(QUEUE_SECONDS));
                    return GoalProgress::Working;
                }
                if self.seek.setback(ctx, |ctx| Self::nearest(ctx, true)) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            // The dock lost its power, or somebody took the side: choose again.
            (TaskResult::Failed, _) => {
                ctx.tasks.clear();
                self.seek.forget();
            }
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        // Walked up to a dock that was taken: choose afresh.
        if let Some(dock) = self.seek.target
            && Self::has_room(ctx, dock)
            && self.plan(ctx)
        {
            return GoalProgress::Working;
        }
        self.choose(ctx)
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("dock", self.seek.target.map_or("none".to_string(), |dock| format!("{}, {}", dock.x, dock.y)))]
    }
}
