//! [`SalvageGoal`]: carry out an order to salvage a heap of scrap.
//!
//! An errand: a unit goes to a heap because it was told to
//! ([`crate::sim::Command::Salvage`]) — a scrap collector goes for a living,
//! and that is [`CollectGoal`](super::CollectGoal) — and the order is what it
//! remembers ([`SALVAGE_ORDER`]):
//! [`OrdersRoutine`](crate::sim::brain::routines::OrdersRoutine) wants this
//! goal for as long as the order stands, and this goal lets the order go —
//! when the heap is empty, when the unit can carry no more, or when there is
//! no getting to it. A need that takes over meanwhile puts it down, and the
//! order is still there when the need is met.
//!
//! The plan is [`MoveTo`](crate::sim::brain::tasks::MoveTo) beside the heap
//! and [`Salvage`](crate::sim::brain::tasks::Salvage), one piece a time. A
//! piece lands in the pack at the end of the tick the work finished in (the
//! world step's doing), so the tick after, the heap and the pack already say
//! whether to work another: the goal plans from them, and never counts.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::memory::{Recall, SALVAGE_ORDER};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Seek, Stand};

/// Twenty minutes to work one piece loose from a heap — ten seconds of
/// watching it, written in world time like every duration a body is watched
/// standing through ([`crate::sim::clock`]).
pub const SALVAGE_SECONDS: f32 = watched(20.0 * MINUTE);

/// Salvage the heap of scrap a unit was told to.
///
/// What it remembers is how the way to the heap has gone; the heap itself is
/// the order, in memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SalvageGoal {
    seek: Seek,
}

impl SalvageGoal {
    pub fn new() -> SalvageGoal {
        SalvageGoal::default()
    }

    /// The heap it was told to salvage, if an order stands.
    fn order(ctx: &GoalCtx<'_>) -> Option<Point> {
        match ctx.memory.get(SALVAGE_ORDER) {
            Some(Recall::Cell(heap)) => Some(*heap),
            _ => None,
        }
    }

    /// Queue the walk beside the heap (unless already there) and a piece's
    /// work, on the back of the queue. `false` when nowhere beside it can be
    /// stood on.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>, heap: Point) -> bool {
        if self.seek.target != Some(heap) {
            self.seek.choose(heap);
        }
        match stand_beside(ctx, heap) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::salvage(heap, SALVAGE_SECONDS));
        true
    }

    /// Let the order go, and say why.
    fn stop(&mut self, ctx: &mut GoalCtx<'_>, heap: Point, why: &str) {
        let _ = ctx.memory.forget(SALVAGE_ORDER);
        self.seek.forget();
        ctx.think.log.unit(ctx.body.uid(), format!(
            "{} stopped salvaging at {}, {}: {why}",
            ctx.body.uid(),
            heap.x,
            heap.y
        ));
    }
}

impl GoalExecutor for SalvageGoal {
    fn goal(&self) -> GoalId {
        GoalId::Salvage
    }

    fn save(&self) -> pb::Executor {
        let SalvageGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// Picked back up: the half-walked route is gone, the order is not, and
    /// [`SalvageGoal::process`] plans from where the unit stands.
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.seek.steady();
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let Some(heap) = Self::order(ctx) else {
            self.seek.forget();
            return GoalProgress::Achieved;
        };
        let Some(room) = ctx.inventory.map(|inventory| inventory.room_for(ItemKind::Scrap)) else {
            // A kind that carries nothing.
            self.stop(ctx, heap, "it has nothing to carry scrap in");
            return GoalProgress::Blocked;
        };

        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                // A body in the way: wait and try again. No way there: there
                // is no other heap it was told to go to, so the order goes.
                if self.seek.setback(ctx, |_| None) && self.plan(ctx, heap) {
                    return GoalProgress::Working;
                }
                self.stop(ctx, heap, "there is no getting to it");
                return GoalProgress::Blocked;
            }
            // The work stopped short — the heap emptied under it, the pack
            // filled, or it was pulled away: worked out again below, from
            // what is true now.
            (TaskResult::Failed, _) => ctx.tasks.clear(),
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }

        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }

        // Nothing queued: another piece, or done.
        if ctx.think.map.scrap(heap).is_none() {
            self.stop(ctx, heap, "nothing left");
            return GoalProgress::Achieved;
        }
        if !room {
            self.stop(ctx, heap, "it can carry no more");
            return GoalProgress::Achieved;
        }
        if self.plan(ctx, heap) {
            GoalProgress::Working
        } else {
            self.stop(ctx, heap, "nowhere to stand beside it");
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "heap",
            match self.seek.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "none".to_string(),
            },
        )]
    }
}
