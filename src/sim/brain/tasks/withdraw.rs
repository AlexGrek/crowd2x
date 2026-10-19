//! [`Withdraw`]: fill the pack from a store with what it holds.
//!
//! [`Deposit`](super::Deposit)'s other half: from beside the store, a few
//! minutes' work, and as many pieces as the pack has room for come out. A
//! courier fills up with scrap at the warehouse's bins and with steel at the
//! factory's pallets (`HaulGoal`); a steelworker with scrap at a hopper
//! (`SmeltGoal`). The store is the world's, and two units can finish on its
//! last pieces in one tick, so the task *asks* ([`Effect::Withdraw`]) and the
//! world step hands the pieces out, in slot order (`sim::world_step`).
//!
//! It is work, and tires like it ([`Event::Worked`]): for the time it took,
//! or for as much as was done of it if it is cut short.

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

/// Five minutes of world to fill a pack from a store.
pub const WITHDRAW_SECONDS: f32 = watched(5.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Withdraw {
    /// The store's cell: what the unit is turned towards, never where it
    /// stands.
    pub store: Point,
    /// How long filling the pack takes, in **watched** seconds.
    pub seconds: f32,
}

impl Withdraw {
    /// Whether it can go on: one step from the store, the store still there
    /// with something in it, and room in the pack for a piece of it.
    fn can_withdraw(&self, ctx: &TaskCtx<'_>) -> bool {
        let Some(store) = ctx.think.bins.store(self.store) else {
            return false;
        };
        ctx.here().manhattan_distance(self.store) <= 1
            && ctx.think.features.has(store.feature(), self.store)
            && ctx.think.bins.stored(self.store) > 0
            && ctx.inventory.as_deref().is_some_and(|inventory| inventory.room_for(store.item()))
    }
}

impl TaskExecutor for Withdraw {
    /// **Checked every tick**: a unit pulled away, a store somebody else
    /// emptied meanwhile or a pack with no room ends the work there, and
    /// nothing is asked for. Finishing asks the world for the pieces
    /// ([`Effect::Withdraw`]) and tells the body it worked.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_withdraw(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.store, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::Withdraw { at: self.store };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked {
                        world_seconds: self.seconds * TIME_SCALE,
                    });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before the pack was filled: nothing came out, but the
    /// time spent at it was still work.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked {
                world_seconds: elapsed * TIME_SCALE,
            });
        }
    }

    fn describe(&self) -> String {
        format!("take from the store at {}, {}", self.store.x, self.store.y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::item::ItemKind;

    const PALLET: Point = Point::new(6, 6);

    /// A unit in `cell` of a room with a steel pallet at [`PALLET`] holding
    /// `stored` ingots.
    fn rig_at(cell: Point, stored: u32) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "steel pallet", PALLET);
        let mut rig = Rig::new(map, cell);
        rig.biology.edit(|stats| stats.with_stamina(80.0).with_fun(40.0));
        rig.world.bins = crate::sim::Bins::from_map(&rig.world.map);
        let _ = rig.world.bins.add(PALLET, stored);
        rig
    }

    #[test]
    fn taking_from_across_the_room_fails_without_starting() {
        let mut rig = rig_at(Point::new(1, 1), 4);
        let mut task = Task::withdraw(PALLET, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }

    /// The world is asked for the pieces once the work is over, and it tires.
    #[test]
    fn the_world_is_asked_for_the_pieces_once_the_work_is_over() {
        let mut rig = rig_at(Point::new(5, 6), 4);
        let mut task = Task::withdraw(PALLET, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.action.interacting_with(), Some(PALLET));
        assert_eq!(rig.effect, Effect::None, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Withdraw { at: PALLET });
        assert!(rig.biology.stats().stamina() < 80.0, "fetching is work");
    }

    #[test]
    fn an_empty_store_or_a_full_pack_is_nothing_to_take() {
        let mut rig = rig_at(Point::new(5, 6), 0);
        assert_eq!(rig.tick(&mut Task::withdraw(PALLET, 0.5)), TaskResult::Failed);

        let mut rig = rig_at(Point::new(5, 6), 4);
        while rig.inventory.stow(ItemKind::Steel) {}
        assert_eq!(rig.tick(&mut Task::withdraw(PALLET, 0.5)), TaskResult::Failed);
    }

    /// What the world does with the ask: as many as the pack has room for,
    /// and no more than the store has.
    #[test]
    fn the_world_hands_out_as_many_as_fit_and_the_store_has() {
        let mut rig = rig_at(Point::new(5, 6), 3);
        let mut task = Task::withdraw(PALLET, 0.5);
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        let lines = rig.apply_effect();
        assert_eq!(rig.inventory.count(ItemKind::Steel), 2, "two ingots are as heavy as a pack takes");
        assert_eq!(rig.world.bins.stored(PALLET), 1);
        assert!(lines.iter().any(|line| line.contains("took 2 steel from the steel pallet at 6, 6: 1 left")), "{lines:?}");
    }
}
