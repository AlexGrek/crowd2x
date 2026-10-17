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

use crate::map::Point;
use crate::sim::item::ItemKind;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Salvage {
    /// The heap's cell: what the unit is turned towards, never where it
    /// stands.
    pub heap: Point,
    /// How long working one piece loose takes.
    pub seconds: f32,
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
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("salvage the scrap at {}, {}", self.heap.x, self.heap.y)
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

    #[test]
    fn a_full_pack_cannot_take_another_piece() {
        let (mut rig, heap) = rig_beside_heap(3, 1);
        while rig.inventory.stow(ItemKind::Scrap) {}
        let mut task = Task::salvage(heap, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }
}
