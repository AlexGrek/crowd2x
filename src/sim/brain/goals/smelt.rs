//! [`SmeltGoal`]: a steelworker's shift — scrap metal out of the steel
//! factory's hoppers, smelted into steel at its own furnace, and the steel
//! onto the factory's pallets.
//!
//! One ingot on a pallet is the goal's unit of work, as a packful in the bin
//! is `CollectGoal`'s: it reports `Achieved` when the steel is stored, and the
//! routine (`WorkRoutine`, while the shift is due) starts the next. **It plans
//! from what is true now** and never counts:
//!
//! * **enough scrap in the pack for an ingot**
//!   ([`SCRAP_PER_STEEL`](crate::sim::item::SCRAP_PER_STEEL)) — to its own
//!   furnace (the job's workplace) and [`Smelt`](crate::sim::brain::tasks::Smelt);
//! * **steel in the pack** — to the nearest steel pallet and
//!   [`Deposit`](crate::sim::brain::tasks::Deposit) it;
//! * **neither** — to the nearest scrap hopper with scrap in it and
//!   [`Withdraw`](crate::sim::brain::tasks::Withdraw) a packful; with none,
//!   nothing to do: `Blocked`, and the goal's back-off spaces out the next
//!   look. The couriers fill the hoppers (`HaulGoal`).
//!
//! What it does once it is beside its target follows from what the target
//! is — its furnace, a pallet or a hopper — so a seek is all it remembers.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::{ItemKind, SCRAP_PER_STEEL};

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{nearest_store, stand_beside, Seek, Stand};
use crate::sim::brain::tasks::{DEPOSIT_SECONDS, SMELT_SECONDS, WITHDRAW_SECONDS};

/// Smelt steel for a living.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SmeltGoal {
    seek: Seek,
}

impl SmeltGoal {
    pub fn new() -> SmeltGoal {
        SmeltGoal::default()
    }

    /// The steelworker's own furnace, if it works: the job's workplace, and
    /// a furnace there with power.
    fn furnace(ctx: &GoalCtx<'_>) -> Option<Point> {
        let at = ctx.job?.workplace;
        ctx.think.features.has(FeatureKind::Furnace, at).then_some(at)
    }

    /// The work to do once beside `target`: smelt at the furnace, store
    /// steel on a pallet, fetch scrap from a hopper.
    fn work_at(ctx: &GoalCtx<'_>, target: Point) -> Task {
        if ctx.think.features.has(FeatureKind::SteelPallet, target) {
            Task::deposit(target, DEPOSIT_SECONDS)
        } else if ctx.think.features.has(FeatureKind::ScrapHopper, target) {
            Task::withdraw(target, WITHDRAW_SECONDS)
        } else {
            Task::smelt(target, SMELT_SECONDS)
        }
    }

    /// Head for `place` (unless already on the way), queue the walk beside
    /// it (unless already there) and the work there. `false` when nowhere
    /// beside it can be stood on.
    fn go(&mut self, ctx: &mut GoalCtx<'_>, place: Point) -> bool {
        if self.seek.target != Some(place) {
            self.seek.choose(place);
        }
        self.plan(ctx)
    }

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
        let _ = ctx.tasks.push_back(Self::work_at(ctx, target));
        true
    }

    /// Whether `target` is still somewhere to go: its own furnace working, a
    /// pallet, or a hopper with scrap in it.
    fn still_worth_it(ctx: &GoalCtx<'_>, target: Point) -> bool {
        match ctx.job {
            Some(job) if job.workplace == target => Self::furnace(ctx).is_some(),
            _ => {
                ctx.think.features.has(FeatureKind::SteelPallet, target)
                    || (ctx.think.features.has(FeatureKind::ScrapHopper, target) && ctx.think.bins.stored(target) > 0)
            }
        }
    }
}

impl GoalExecutor for SmeltGoal {
    fn goal(&self) -> GoalId {
        GoalId::Smelt
    }

    fn save(&self) -> pb::Executor {
        let SmeltGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: a target no longer worth going to — a hopper emptied
    /// meanwhile, a furnace that is not the job's any more — is forgotten;
    /// [`SmeltGoal::process`] plans the rest from where it stands.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        match self.seek.target {
            Some(target) if Self::still_worth_it(ctx, target) => self.seek.steady(),
            Some(_) => self.seek.forget(),
            None => {}
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(job), Some(inventory)) = (ctx.biology, ctx.job, ctx.inventory) else {
            return GoalProgress::Blocked;
        };
        let furnace = job.workplace;

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // next nearest of the same — but no other furnace is its own.
                let retry = match self.seek.target {
                    Some(target) if target == furnace => self.seek.setback(ctx, |_| None),
                    Some(target) if ctx.think.features.has(FeatureKind::SteelPallet, target) => {
                        self.seek.setback(ctx, |ctx| nearest_store(ctx, FeatureKind::SteelPallet, false))
                    }
                    _ => self.seek.setback(ctx, |ctx| nearest_store(ctx, FeatureKind::ScrapHopper, true)),
                };
                if retry && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            // The hopper emptied under it, the furnace lost its power, it was
            // pulled away: worked out again below, from what is true now.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Deposit(_))) => {
                // Steel on the pallet: one piece of work done.
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        let (scrap, steel) = (inventory.count(ItemKind::Scrap), inventory.count(ItemKind::Steel));
        let next = if scrap >= SCRAP_PER_STEEL {
            Self::furnace(ctx)
        } else if steel > 0 {
            nearest_store(ctx, FeatureKind::SteelPallet, false)
        } else {
            let hopper = nearest_store(ctx, FeatureKind::ScrapHopper, true);
            if hopper.is_none() {
                // Nothing to smelt: held off, and the hold doubling is what
                // spaces these lines out.
                ctx.think.log.unit(ctx.body.uid(), format!("{} found no scrap to smelt", ctx.body.uid()));
            }
            hopper
        };
        match next {
            Some(place) if self.go(ctx, place) => GoalProgress::Working,
            _ => {
                self.seek.forget();
                GoalProgress::Blocked
            }
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
