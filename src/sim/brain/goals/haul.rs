//! A courier's shift: carry input to a factory hopper and output from
//! factory pallets to the warehouse. **Every courier is one pool**: the steel
//! courier, the food courier and the store's courier all serve both chains —
//! scrap and steel, wheat and meals — and what a trip is follows from what the
//! pack holds and what is waiting, never from the job. Any of them the courier
//! dispatch sends for the store's food puts this down for `DeliverGoal` until
//! the food is in (`sim::dispatch`).
//!
//! Plans from the current pack after every handover. Empty couriers take the
//! chain whose hoppers hold the least (ties to food), from the nearer stocked
//! source of it, favoring output on ties. One deposited load is one achieved
//! trip; Seek supplies bounded retries and back-off.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{nearest_store, stand_beside, Seek, Stand};
use crate::sim::brain::tasks::{DEPOSIT_SECONDS, WITHDRAW_SECONDS};

/// The stores and goods of one chain.
#[derive(Clone, Copy)]
struct Route {
    input: ItemKind,
    output: ItemKind,
    source: FeatureKind,
    hopper: FeatureKind,
    pallet: FeatureKind,
    rack: FeatureKind,
}

/// Wheat from the farm to the food factory, meals from it to the warehouse.
const FOOD: Route = Route {
    input: ItemKind::Wheat,
    output: ItemKind::Meal,
    source: FeatureKind::GrainBin,
    hopper: FeatureKind::WheatHopper,
    pallet: FeatureKind::MealPallet,
    rack: FeatureKind::MealRack,
};

/// Scrap from the warehouse to the steel factory, steel from it back.
const STEEL: Route = Route {
    input: ItemKind::Scrap,
    output: ItemKind::Steel,
    source: FeatureKind::ScrapBin,
    hopper: FeatureKind::ScrapHopper,
    pallet: FeatureKind::SteelPallet,
    rack: FeatureKind::SteelRack,
};

const ROUTES: [Route; 2] = [FOOD, STEEL];

/// Carry factory inputs and outputs for a living.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HaulGoal {
    seek: Seek,
}

impl HaulGoal {
    pub fn new() -> HaulGoal {
        HaulGoal::default()
    }

    /// The work to do once beside `target`: a hopper or a rack is somewhere
    /// to unload, a bin or a pallet somewhere to fill up.
    fn work_at(ctx: &GoalCtx<'_>, target: Point) -> Task {
        let features = ctx.think.features;
        if ROUTES.iter().any(|route| features.has(route.hopper, target) || features.has(route.rack, target)) {
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

    /// The nearer stocked source of the first chain with one, the chain whose
    /// hoppers hold least first (food on a tie) so neither factory starves the
    /// other; within it, ties favor the factory output.
    fn somewhere_to_fill_up(ctx: &GoalCtx<'_>) -> Option<Point> {
        let here = ctx.body.center_position();
        let bins = ctx.think.bins;
        let mut routes = ROUTES;
        routes.sort_by_key(|route| bins.total_in(route.hopper));
        routes.into_iter().find_map(|route| {
            let output = nearest_store(ctx, route.pallet, true);
            let input = nearest_store(ctx, route.source, true);
            [output, input].into_iter().flatten().min_by_key(|&cell| here.manhattan_distance(cell))
        })
    }

    /// Where what the pack holds goes: meals and steel to the nearest rack,
    /// wheat and scrap to the nearest hopper. Nothing carried, nothing.
    fn somewhere_to_unload(ctx: &GoalCtx<'_>, pack: &crate::sim::inventory::Inventory) -> Option<Point> {
        ROUTES.iter().find_map(|route| {
            if pack.count(route.output) > 0 {
                Some(nearest_store(ctx, route.rack, false))
            } else if pack.count(route.input) > 0 {
                Some(nearest_store(ctx, route.hopper, false))
            } else {
                None
            }
        })?
    }

    /// Whether `target` is still somewhere to go: a hopper or a rack, or a
    /// bin or pallet with something in it.
    fn still_worth_it(ctx: &GoalCtx<'_>, target: Point) -> bool {
        let features = ctx.think.features;
        ROUTES.iter().any(|route| {
            features.has(route.rack, target)
                || features.has(route.hopper, target)
                || ((features.has(route.source, target) || features.has(route.pallet, target)) && ctx.think.bins.stored(target) > 0)
        })
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
    /// emptied meanwhile — is forgotten; [`HaulGoal::process`] plans the rest
    /// from where it stands.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        match self.seek.target {
            Some(target) if Self::still_worth_it(ctx, target) => self.seek.steady(),
            Some(_) => self.seek.forget(),
            None => {}
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let (Some(_), Some(_), Some(inventory)) = (ctx.biology, ctx.job, ctx.inventory) else {
            return GoalProgress::Blocked;
        };

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: the
                // next nearest of the same.
                let features = ctx.think.features;
                let unloading = self.seek.target.and_then(|target| {
                    ROUTES.iter().flat_map(|route| [route.rack, route.hopper]).find(|&kind| features.has(kind, target))
                });
                let retry = match unloading {
                    Some(kind) => self.seek.setback(ctx, move |ctx| nearest_store(ctx, kind, false)),
                    None => self.seek.setback(ctx, Self::somewhere_to_fill_up),
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
        let next = match Self::somewhere_to_unload(ctx, inventory) {
            Some(place) => Some(place),
            None if ROUTES.iter().any(|route| inventory.count(route.output) > 0 || inventory.count(route.input) > 0) => None,
            None => {
                let place = Self::somewhere_to_fill_up(ctx);
                if place.is_none() {
                    // Nothing to carry: held off, and the hold doubling is what
                    // spaces these lines out.
                    ctx.think.log.unit(ctx.body.uid(), format!("{} found nothing to carry", ctx.body.uid()));
                }
                place
            }
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
