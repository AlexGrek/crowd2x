//! [`Deposit`]: empty the pack into a store — whatever of the one thing the
//! store holds ([`Store::item`](crate::sim::bins::Store::item)).
//!
//! The end of a scrap collector's trip (`CollectGoal`): from beside its bin,
//! a few minutes' work, and every piece it carries goes in. The same for a
//! courier's scrap into a hopper and steel onto the warehouse's rack
//! (`HaulGoal`), and a steelworker's steel onto a pallet (`SmeltGoal`). The pack is the
//! unit's own and is emptied here, when the work finishes; the bin is the
//! world's, so the pieces are *asked* in ([`Effect::Deposit`]) and the world
//! step puts them there (`sim::world_step`), as a heap's piece is asked out
//! by [`Salvage`](super::Salvage).
//!
//! It is work, and tires like it ([`Event::Worked`]): for the time it took,
//! or for as much as was done of it if it is cut short.

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

/// Five minutes of world to empty a pack into a bin.
pub const DEPOSIT_SECONDS: f32 = watched(5.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Deposit {
    /// The bin's cell: what the unit is turned towards, never where it
    /// stands.
    pub bin: Point,
    /// How long emptying the pack takes, in **watched** seconds.
    pub seconds: f32,
}

impl Deposit {
    /// Whether it can go on: one step from the store, the store still there,
    /// and some of what it holds in the pack to put in it.
    fn can_deposit(&self, ctx: &TaskCtx<'_>) -> bool {
        let Some(store) = ctx.think.bins.store(self.bin) else {
            return false;
        };
        ctx.here().manhattan_distance(self.bin) <= 1
            && ctx.think.features.has(store.feature(), self.bin)
            && ctx.inventory.as_deref().is_some_and(|inventory| inventory.count(store.item()) > 0)
    }
}

impl TaskExecutor for Deposit {
    /// **Checked every tick**: a unit pulled away, a bin gone or a pack with
    /// nothing in it ends the work there, and nothing is asked for.
    /// Finishing takes every piece out of the pack, asks the world to put
    /// them in the bin ([`Effect::Deposit`]) and tells the body it worked.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_deposit(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.bin, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                let (Some(inventory), Some(item)) = (ctx.inventory.as_deref_mut(), ctx.think.bins.item(self.bin)) else {
                    return TaskResult::Failed;
                };
                let mut pieces: u8 = 0;
                while inventory.take_out(item) {
                    pieces += 1;
                }
                *ctx.effect = Effect::Deposit { at: self.bin, pieces };
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

    /// Pulled away before the pack was empty: nothing went in, but the time
    /// spent at it was still work.
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
        format!("store what it carries at {}, {}", self.bin.x, self.bin.y)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::item::ItemKind;

    const BIN: Point = Point::new(6, 6);

    /// A unit in `cell` of a room with a scrap bin at [`BIN`], carrying
    /// `pieces` of scrap metal.
    fn rig_at(cell: Point, pieces: u8) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "scrap bin", BIN);
        let mut rig = Rig::new(map, cell);
        rig.biology.edit(|stats| stats.with_stamina(80.0).with_fun(40.0));
        for _ in 0..pieces {
            assert!(rig.inventory.stow(ItemKind::Scrap));
        }
        rig
    }

    #[test]
    fn storing_scrap_from_across_the_room_fails_without_starting() {
        let mut rig = rig_at(Point::new(1, 1), 3);
        let mut task = Task::deposit(BIN, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.inventory.count(ItemKind::Scrap), 3);
    }

    /// The pack is emptied only once the work is over, all of it at once,
    /// and the world is asked to put that many in the bin. It tires, and is
    /// no fun.
    #[test]
    fn the_pack_is_emptied_once_the_work_is_over_and_the_bin_asked_to_take_every_piece() {
        let mut rig = rig_at(Point::new(5, 6), 3);
        let mut task = Task::deposit(BIN, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.action.interacting_with(), Some(BIN));
        assert_eq!(rig.inventory.count(ItemKind::Scrap), 3, "not yet");
        assert_eq!(rig.effect, Effect::None);
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.inventory.count(ItemKind::Scrap), 0);
        assert_eq!(rig.effect, Effect::Deposit { at: BIN, pieces: 3 });
        assert!(rig.biology.stats().stamina() < 80.0, "storing scrap is work");
        assert_eq!(rig.biology.stats().fun(), 40.0, "and no fun");
        assert_eq!(rig.walk.body().center_position(), Point::new(5, 6), "never on the bin");
    }

    #[test]
    fn nothing_to_store_is_not_a_trip_to_the_bin() {
        let mut rig = rig_at(Point::new(5, 6), 0);
        let mut task = Task::deposit(BIN, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.effect, Effect::None);
    }

    #[test]
    fn a_bin_that_is_gone_ends_the_work_with_the_pack_still_full() {
        let mut rig = rig_at(Point::new(5, 6), 2);
        let mut task = Task::deposit(BIN, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.world.features = crate::sim::feature::Features::default();
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.inventory.count(ItemKind::Scrap), 2);
        assert_eq!(rig.effect, Effect::None);
    }

    #[test]
    fn walking_away_mid_work_fails_it_and_the_scrap_stays_in_the_pack() {
        let mut rig = rig_at(Point::new(5, 6), 1);
        let mut task = Task::deposit(BIN, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.inventory.count(ItemKind::Scrap), 1);
    }
}
