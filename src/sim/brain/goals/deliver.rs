//! [`DeliverGoal`]: bring a unit of food from the warehouse to the store's
//! superfridge — what a courier the courier dispatch sent does
//! ([`crate::sim::dispatch`]).
//!
//! Raised by `WorkRoutine` over the courier's own job while the dispatch says
//! the order is its (`Think::dispatch`). From what is true now: **a meal in
//! the pack** → beside the order's superfridge, and
//! [`Deposit`](crate::sim::brain::tasks::Deposit) it — the world closes the
//! order (`Effect::Deposit`) and the goal is `Achieved`; **none** → beside the
//! nearest meal rack with a meal on it — the warehouse's — and
//! [`Fetch`](crate::sim::brain::tasks::Fetch) one. No meal anywhere is
//! `Blocked`, and the dispatch takes the order back.
//!
//! Kept the way [`HaulGoal`](super::HaulGoal) keeps a trip: where it is
//! going, in a [`Seek`].

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::{DEPOSIT_SECONDS, WITHDRAW_SECONDS};
use super::{nearest_store, stand_beside, Seek, Stand};

/// Take one unit of food to the superfridge the dispatch sent it for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DeliverGoal {
    seek: Seek,
}

impl DeliverGoal {
    pub fn new() -> DeliverGoal {
        DeliverGoal::default()
    }

    /// The superfridge its order is for, while it has one.
    fn store(ctx: &GoalCtx<'_>) -> Option<Point> {
        ctx.think.dispatch.order_of(ctx.body.uid()).map(|order| order.store)
    }

    fn carries_food(ctx: &GoalCtx<'_>) -> bool {
        ctx.inventory.is_some_and(|pack| pack.count(ItemKind::Meal) > 0)
    }

    /// Queue the walk beside the target (unless already there) and the work
    /// there: the delivery at the superfridge, the fetch anywhere else.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(target) = self.seek.target else {
            return false;
        };
        match stand_beside(ctx, target) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let task = if Self::store(ctx) == Some(target) {
            Task::deposit(target, DEPOSIT_SECONDS)
        } else {
            Task::fetch(target, WITHDRAW_SECONDS)
        };
        let _ = ctx.tasks.push_back(task);
        true
    }

    /// Whether `target` is still somewhere to go: the order's superfridge
    /// with food in the pack, or a meal rack with a meal on it without.
    fn still_worth_it(ctx: &GoalCtx<'_>, target: Point) -> bool {
        if Self::carries_food(ctx) {
            Self::store(ctx) == Some(target)
        } else {
            ctx.think.features.has(FeatureKind::MealRack, target) && ctx.think.bins.stored(target) > 0
        }
    }
}

impl GoalExecutor for DeliverGoal {
    fn goal(&self) -> GoalId {
        GoalId::Deliver
    }

    fn save(&self) -> pb::Executor {
        let DeliverGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: a rack emptied meanwhile, or an order gone, is
    /// forgotten; [`DeliverGoal::process`] plans the rest from where it
    /// stands.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        match self.seek.target {
            Some(target) if Self::still_worth_it(ctx, target) => self.seek.steady(),
            Some(_) => self.seek.forget(),
            None => {}
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(store)) = (ctx.biology, Self::store(ctx)) else {
            return GoalProgress::Blocked;
        };

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // next rack along — but there is one superfridge it is for.
                let retry = match self.seek.target {
                    Some(target) if target == store => self.seek.setback(ctx, |_| None),
                    _ => self.seek.setback(ctx, |ctx| nearest_store(ctx, FeatureKind::MealRack, true)),
                };
                if retry && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            // The rack emptied under it, it was pulled away: worked out
            // again below.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Deposit(_))) => {
                // The food is in: the world has closed the order.
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::Fetch(_))) => self.seek.forget(),
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        let next = if Self::carries_food(ctx) {
            Some(store)
        } else {
            let rack = nearest_store(ctx, FeatureKind::MealRack, true);
            if rack.is_none() {
                ctx.think.log.unit(ctx.body.uid(), format!("{} found no food in the warehouse for the store", ctx.body.uid()));
            }
            rack
        };
        let Some(place) = next else {
            self.seek.forget();
            return GoalProgress::Blocked;
        };
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.seek.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "going to",
            match self.seek.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "none".to_string(),
            },
        )]
    }
}
