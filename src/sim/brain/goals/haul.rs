//! [`HaulGoal`]: a courier's shift — scrap metal from the warehouse's bins to
//! its own hopper in the steel factory, and steel from the factory's pallets
//! to the warehouse's rack.
//!
//! One load stored is the goal's unit of work: it reports `Achieved` when a
//! packful is in a hopper or on the rack, and the routine (`WorkRoutine`,
//! while the shift is due) starts the next. **It plans from what is true
//! now** and never counts:
//!
//! * **steel in the pack** — to the nearest steel rack and
//!   [`Deposit`](crate::sim::brain::tasks::Deposit) it;
//! * **scrap in the pack** — to its own hopper (the job's workplace) and
//!   deposit it;
//! * **an empty pack** — to the nearer of the nearest steel pallet with steel
//!   on it and the nearest scrap bin with scrap in it, and
//!   [`Withdraw`](crate::sim::brain::tasks::Withdraw) a packful; with neither,
//!   nothing to carry: `Blocked`.
//!
//! **The trips alternate of themselves**: a load of scrap left in the
//! factory's hopper leaves the courier beside the factory's pallets, the
//! nearer of the two places to fill up, and a load of steel on the
//! warehouse's rack leaves it beside the bins. Nobody tells it which way.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{nearest_store, stand_beside, Seek, Stand};
use crate::sim::brain::tasks::{DEPOSIT_SECONDS, WITHDRAW_SECONDS};

/// Carry scrap and steel for a living.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HaulGoal {
    seek: Seek,
}

impl HaulGoal {
    pub fn new() -> HaulGoal {
        HaulGoal::default()
    }

    /// The courier's own hopper, if it is there: the job's workplace.
    fn hopper(ctx: &GoalCtx<'_>) -> Option<Point> {
        let at = ctx.job?.workplace;
        ctx.think.features.has(FeatureKind::ScrapHopper, at).then_some(at)
    }

    /// The work to do once beside `target`: a hopper or a rack is somewhere
    /// to unload, a bin or a pallet somewhere to fill up.
    fn work_at(ctx: &GoalCtx<'_>, target: Point) -> Task {
        let features = ctx.think.features;
        if features.has(FeatureKind::ScrapHopper, target) || features.has(FeatureKind::SteelRack, target) {
            Task::deposit(target, DEPOSIT_SECONDS)
        } else {
            Task::withdraw(target, WITHDRAW_SECONDS)
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

    /// The nearer of somewhere to fill up with steel and somewhere to fill
    /// up with scrap. Ties to the steel, then the lower cell.
    fn somewhere_to_fill_up(ctx: &GoalCtx<'_>) -> Option<Point> {
        let here = ctx.body.center_position();
        let steel = nearest_store(ctx, FeatureKind::SteelPallet, true);
        let scrap = nearest_store(ctx, FeatureKind::ScrapBin, true);
        [steel, scrap].into_iter().flatten().min_by_key(|&cell| here.manhattan_distance(cell))
    }

    /// Whether `target` is still somewhere to go: its own hopper, a rack, or
    /// a bin or pallet with something in it.
    fn still_worth_it(ctx: &GoalCtx<'_>, target: Point) -> bool {
        let features = ctx.think.features;
        match ctx.job {
            Some(job) if job.workplace == target => Self::hopper(ctx).is_some(),
            _ => {
                features.has(FeatureKind::SteelRack, target)
                    || ((features.has(FeatureKind::ScrapBin, target) || features.has(FeatureKind::SteelPallet, target))
                        && ctx.think.bins.stored(target) > 0)
            }
        }
    }
}

impl GoalExecutor for HaulGoal {
    fn goal(&self) -> GoalId {
        GoalId::Haul
    }

    fn save(&self) -> pb::Executor {
        let HaulGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: a target no longer worth going to — a bin or pallet
    /// emptied meanwhile, a hopper that is not the job's any more — is
    /// forgotten; [`HaulGoal::process`] plans the rest from where it stands.
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
        let hopper = job.workplace;

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // next nearest of the same — but no other hopper is its own.
                let features = ctx.think.features;
                let retry = match self.seek.target {
                    Some(target) if target == hopper => self.seek.setback(ctx, |_| None),
                    Some(target) if features.has(FeatureKind::SteelRack, target) => {
                        self.seek.setback(ctx, |ctx| nearest_store(ctx, FeatureKind::SteelRack, false))
                    }
                    _ => self.seek.setback(ctx, Self::somewhere_to_fill_up),
                };
                if retry && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            // A bin or pallet emptied under it, the pack filled, it was
            // pulled away: worked out again below, from what is true now.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::Deposit(_))) => {
                // A load stored: one trip done.
                self.seek.forget();
                return GoalProgress::Achieved;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        let next = if inventory.count(ItemKind::Steel) > 0 {
            nearest_store(ctx, FeatureKind::SteelRack, false)
        } else if inventory.count(ItemKind::Scrap) > 0 {
            Self::hopper(ctx)
        } else {
            let place = Self::somewhere_to_fill_up(ctx);
            if place.is_none() {
                // Nothing to carry: held off, and the hold doubling is what
                // spaces these lines out.
                ctx.think.log.unit(ctx.body.uid(), format!("{} found nothing to carry", ctx.body.uid()));
            }
            place
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
