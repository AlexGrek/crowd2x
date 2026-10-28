//! A courier's shift: carry input to its factory hopper and output from
//! factory pallets to the warehouse. Steel couriers serve scrap/steel;
//! food couriers serve farm wheat/meals, and so does the store's courier,
//! whose own workplace is the store's superfridge: it carries its wheat to
//! the nearest wheat hopper. The route follows the job, so the chains share
//! transport behavior without exchanging their goods. Any of them the
//! courier dispatch sends for the store's food puts this down for
//! `DeliverGoal` until the food is in (`sim::dispatch`).
//!
//! Plans from the current pack after every handover. Empty couriers choose
//! the nearer stocked source, favoring output on ties. One deposited load
//! is one achieved trip; Seek supplies bounded retries and back-off.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;
use crate::sim::job::JobKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{nearest_store, stand_beside, Seek, Stand};
use crate::sim::brain::tasks::{DEPOSIT_SECONDS, WITHDRAW_SECONDS};

/// The stores and goods served by a courier's workplace.
#[derive(Clone, Copy)]
struct Route {
    input: ItemKind,
    output: ItemKind,
    source: FeatureKind,
    hopper: FeatureKind,
    pallet: FeatureKind,
    rack: FeatureKind,
}

impl Route {
    fn of(ctx: &GoalCtx<'_>) -> Self {
        if ctx.job.is_some_and(|job| matches!(job.kind, JobKind::FoodCourier | JobKind::StoreCourier)) {
            Self { input: ItemKind::Wheat, output: ItemKind::Meal, source: FeatureKind::GrainBin,
                hopper: FeatureKind::WheatHopper, pallet: FeatureKind::MealPallet, rack: FeatureKind::MealRack }
        } else {
            Self { input: ItemKind::Scrap, output: ItemKind::Steel, source: FeatureKind::ScrapBin,
                hopper: FeatureKind::ScrapHopper, pallet: FeatureKind::SteelPallet, rack: FeatureKind::SteelRack }
        }
    }
}

/// Carry factory inputs and outputs for a living.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HaulGoal {
    seek: Seek,
}

impl HaulGoal {
    pub fn new() -> HaulGoal {
        HaulGoal::default()
    }

    /// Where its input goes: its own hopper, the job's workplace, if it is
    /// one — or, for a courier whose workplace is no hopper (the store's),
    /// the nearest of the route's.
    fn hopper(ctx: &GoalCtx<'_>) -> Option<Point> {
        let (at, route) = (ctx.job?.workplace, Route::of(ctx));
        if ctx.think.features.has(route.hopper, at) {
            Some(at)
        } else if ctx.job?.kind == JobKind::StoreCourier {
            nearest_store(ctx, route.hopper, false)
        } else {
            None
        }
    }

    /// The work to do once beside `target`: a hopper or a rack is somewhere
    /// to unload, a bin or a pallet somewhere to fill up.
    fn work_at(ctx: &GoalCtx<'_>, target: Point) -> Task {
        let features = ctx.think.features;
        if features.has(Route::of(ctx).hopper, target) || features.has(Route::of(ctx).rack, target) || features.has(FeatureKind::MealRack, target) {
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

    /// The nearer stocked source; ties favor the factory output.
    fn somewhere_to_fill_up(ctx: &GoalCtx<'_>) -> Option<Point> {
        let here = ctx.body.center_position();
        let route = Route::of(ctx);
        let output = nearest_store(ctx, route.pallet, true);
        let input = nearest_store(ctx, route.source, true);
        [output, input].into_iter().flatten().min_by_key(|&cell| here.manhattan_distance(cell))
    }

    /// Whether `target` is still somewhere to go: its own hopper, a rack, or
    /// a bin or pallet with something in it.
    fn still_worth_it(ctx: &GoalCtx<'_>, target: Point) -> bool {
        let features = ctx.think.features;
        match ctx.job {
            Some(job) if job.workplace == target => Self::hopper(ctx).is_some(),
            _ => {
                features.has(Route::of(ctx).rack, target)
                    || (features.has(FeatureKind::MealRack, target) && ctx.inventory.is_some_and(|pack| pack.count(ItemKind::Meal) > 0))
                    || (features.has(Route::of(ctx).hopper, target) && Self::hopper(ctx) == Some(target))
                    || ((features.has(Route::of(ctx).source, target) || features.has(Route::of(ctx).pallet, target))
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
                    Some(target) if target == hopper || Some(target) == Self::hopper(ctx) => self.seek.setback(ctx, |_| None),
                    Some(target) if features.has(Route::of(ctx).rack, target) => {
                        self.seek.setback(ctx, |ctx| nearest_store(ctx, Route::of(ctx).rack, false))
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

        // A recalled dispatch or a full superfridge can leave any courier
        // with food. Return it before resuming its usual goods route.
        let next = if inventory.count(ItemKind::Meal) > 0 {
            nearest_store(ctx, FeatureKind::MealRack, false)
        } else if inventory.count(Route::of(ctx).output) > 0 {
            nearest_store(ctx, Route::of(ctx).rack, false)
        } else if inventory.count(Route::of(ctx).input) > 0 {
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
