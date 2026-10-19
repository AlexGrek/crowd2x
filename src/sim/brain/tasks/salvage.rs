//! [`Salvage`]: work a heap of scrap out in the wasteland for one piece of
//! scrap metal.
//!
//! A heap is in the way like furniture (`map::wasteland`), so it is worked
//! from **beside** it, the way a computer is used. What finishing earns is not
//! this task's to hand over: the heap is the world's, and two units can finish
//! on its last piece in the same tick, so the task *asks*
//! ([`Effect::Salvage`]) and the world step gives the piece to one of them and
//! stows it in their pack (`sim::world_step`). The goal that queued it hears
//! of it a tick later, when the pack and the heap already say what happened.
//!
//! **Salvaging for a living tires** ([`Salvage::for_work`]): a scrap
//! collector's piece of work is [`Event::Worked`] for the time it took, as a
//! stretch at a desk is (`tasks/work.rs`) — and for as much as was done of it
//! if it is cut short. An order carried out is an errand, and does not.

use crate::map::Point;
use crate::save::{pb, Invalid};
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::TIME_SCALE;
use crate::sim::item::ItemKind;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{self, SavedTask, Task, TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Salvage {
    /// The heap's cell: what the unit is turned towards, never where it
    /// stands.
    pub heap: Point,
    /// How long working one piece loose takes.
    pub seconds: f32,
    /// Whether this is a scrap collector's work ([`Task::salvage_for_work`],
    /// `CollectGoal`) rather than an order ([`Task::salvage`]): work tires
    /// the body, [`Event::Worked`], the way a stretch at a desk does.
    ///
    /// [`Task::salvage_for_work`]: super::super::task::Task::salvage_for_work
    /// [`Task::salvage`]: super::super::task::Task::salvage
    pub for_work: bool,
}

impl Salvage {
    /// Whether it can go on: one step from the heap, something left in it,
    /// and room in the pack for a piece of it.
    fn can_work(&self, ctx: &TaskCtx<'_>) -> bool {
        ctx.here().manhattan_distance(self.heap) <= 1
            && ctx.think.map.scrap(self.heap).is_some()
            && ctx.inventory.as_deref().is_some_and(|inventory| inventory.room_for(ItemKind::Scrap))
    }
}

impl TaskExecutor for Salvage {
    /// **Checked every tick**, like [`UseComputer`](super::UseComputer): a
    /// unit pulled away, a heap somebody else emptied meanwhile or a pack
    /// that filled ends the work there, and nothing is asked for. Finishing
    /// asks the world for the piece ([`Effect::Salvage`]).
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_work(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.heap, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::Salvage { at: self.heap };
                if self.for_work
                    && let Some(biology) = ctx.biology.as_deref_mut()
                {
                    biology.handle(Event::Worked {
                        world_seconds: self.seconds * TIME_SCALE,
                    });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away from a heap worked for a living: what was worked of the
    /// piece still tired the collector, as a stretch at a desk cut short
    /// does. An order owes nothing.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (true, Action::Interact { elapsed, .. }, Some(biology)) = (self.for_work, action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked {
                world_seconds: elapsed * TIME_SCALE,
            });
        }
    }

    fn describe(&self) -> String {
        format!("salvage the scrap at {}, {}", self.heap.x, self.heap.y)
    }
}

impl Task {
    /// Work the heap of scrap in `heap`, from a cell beside it, for one
    /// piece of scrap metal: to an order, so no more tiring than any other
    /// errand.
    pub const fn salvage(heap: Point, seconds: f32) -> Task {
        Task::Salvage(Salvage { heap, seconds, for_work: false })
    }

    /// The same, for a living: a scrap collector's, which tires like a
    /// stretch at a desk ([`Salvage::for_work`]).
    pub const fn salvage_for_work(heap: Point, seconds: f32) -> Task {
        Task::Salvage(Salvage { heap, seconds, for_work: true })
    }
}

/// Saved as `salvage`, or `salvage for work`: the heap and the seconds.
impl SavedTask for Salvage {
    fn save(&self) -> pb::TaskKind {
        task::named(if self.for_work { "salvage for work" } else { "salvage" }, Some(self.heap), self.seconds)
    }

    fn restore(saved: &pb::TaskKind) -> Option<Result<Salvage, Invalid>> {
        let ((cell, seconds), for_work) = match saved {
            pb::TaskKind::Salvage(task) => ((task.cell, task.seconds), false),
            pb::TaskKind::SalvageForWork(task) => ((task.cell, task.seconds), true),
            _ => match task::read_named(saved, "salvage") {
                Some(found) => (found, false),
                None => (task::read_named(saved, "salvage for work")?, true),
            },
        };
        Some(task::cell_and_seconds(cell, seconds, "salvage").map(|(heap, seconds)| Salvage { heap, seconds, for_work }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Bounds, Map, Size, FLOOR};

    /// A unit `away` cells to the left of a heap of `pieces` out in the
    /// wasteland of a small site — with the ground in between loaded, as the
    /// game would have it, though nothing here depends on that.
    fn rig_beside_heap(pieces: u8, away: i32) -> (Rig, Point) {
        let mut map = Map::new(Size::new(8, 8), FLOOR).with_wasteland(3);
        let mut found = None;
        map.heaps(Bounds::new(Point::new(40, -200), Size::new(400, 400)), |cell, heap| {
            let side = cell.offset(-away, 0);
            if found.is_none() && heap.pieces == pieces && (1..=away).all(|step| map.is_passable(cell.offset(-step, 0))) {
                found = Some((cell, side));
            }
        });
        let (heap, side) = found.expect("a heap with ground to its left");
        map.load_chunks(Bounds::new(heap.offset(-40, -40), Size::new(80, 80)));
        (Rig::new(map, side), heap)
    }

    #[test]
    fn working_a_heap_from_beside_it_asks_for_a_piece_once_it_is_done() {
        let (mut rig, heap) = rig_beside_heap(2, 1);
        let mut task = Task::salvage(heap, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Salvage { at: heap });
        // The piece is the world step's to hand over, not the task's.
        assert_eq!(rig.inventory.stowed_items(), 0);
        assert_eq!(rig.walk.body().center_position(), heap.offset(-1, 0), "worked from where it stood");
    }

    #[test]
    fn a_heap_across_the_way_is_not_worked() {
        let (mut rig, heap) = rig_beside_heap(2, 3);
        let mut task = Task::salvage(heap, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }

    #[test]
    fn a_heap_emptied_mid_work_ends_the_work_with_nothing_asked() {
        let (mut rig, heap) = rig_beside_heap(1, 1);
        let mut task = Task::salvage(heap, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.world.map.take_scrap(heap), Some(0), "somebody else got there first");
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.effect, Effect::None);
    }

    /// Salvaging for a living tires the body once the piece is worked loose,
    /// and for the part done when it is cut short; an order does neither.
    #[test]
    fn salvaging_for_a_living_tires_and_an_order_does_not() {
        let (mut rig, heap) = rig_beside_heap(2, 1);
        rig.biology.edit(|stats| stats.with_stamina(80.0));
        let mut ordered = Task::salvage(heap, 0.5);
        assert_eq!(rig.run(&mut ordered, 600), TaskResult::Success);
        assert_eq!(rig.biology.stats().stamina(), 80.0, "an errand is not work");

        let mut work = Task::salvage_for_work(heap, 0.5);
        rig.action = Action::None;
        assert_eq!(rig.run(&mut work, 600), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Salvage { at: heap });
        let after_one = rig.biology.stats().stamina();
        assert!(after_one < 80.0, "a piece worked loose for a living tires: {after_one}");

        // Cut short half way: half the work's worth.
        let mut work = Task::salvage_for_work(heap, 10.0);
        rig.action = Action::None;
        let half = (5.0 / rig.world.dt) as usize;
        for _ in 0..half {
            assert_eq!(rig.tick(&mut work), TaskResult::Executing);
        }
        assert_eq!(rig.biology.stats().stamina(), after_one, "nothing until it is over or cut short");
        work.interrupted(&rig.action, Some(&mut rig.biology));
        let spent = after_one - rig.biology.stats().stamina();
        let whole = crate::sim::biology::energy::WORK_FATIGUE_PER_SECOND * 10.0 * TIME_SCALE;
        assert!((spent - whole / 2.0).abs() < whole * 0.05, "spent {spent}, half is {}", whole / 2.0);

        let mut ordered = Task::salvage(heap, 10.0);
        rig.action = Action::None;
        for _ in 0..half {
            rig.tick(&mut ordered);
        }
        let before = rig.biology.stats().stamina();
        ordered.interrupted(&rig.action, Some(&mut rig.biology));
        assert_eq!(rig.biology.stats().stamina(), before, "an errand cut short owes nothing");
    }

    #[test]
    fn a_full_pack_cannot_take_another_piece() {
        let (mut rig, heap) = rig_beside_heap(3, 1);
        while rig.inventory.stow(ItemKind::Scrap) {}
        let mut task = Task::salvage(heap, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }
}
