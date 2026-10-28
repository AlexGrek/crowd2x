//! [`Fetch`]: take **one** of what a store holds into the pack.
//!
//! [`Withdraw`](super::Withdraw) fills the pack; a courier the dispatch sent
//! for the store's food fetches one meal off the warehouse's meal rack and no
//! more (`DeliverGoal`, [`crate::sim::dispatch`]). From beside the store, a
//! few minutes' work; the store is the world's, so the task *asks*
//! ([`Effect::Fetch`]) and the world step hands the piece out, in slot order.
//!
//! It is work, and tires like it ([`Event::Worked`]): for the time it took,
//! or for as much as was done of it if it is cut short.

use crate::map::Point;
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::TIME_SCALE;
use crate::sim::Effect;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fetch {
    /// The store's cell: what the unit is turned towards, never where it
    /// stands.
    pub store: Point,
    /// How long taking it takes, in **watched** seconds.
    pub seconds: f32,
}

impl Fetch {
    /// Whether it can go on: one step from the store, the store still there
    /// with something in it, and room in the pack for a piece of it.
    fn can_fetch(&self, ctx: &TaskCtx<'_>) -> bool {
        let (Some(store), Some(item)) = (ctx.think.bins.store(self.store), ctx.think.bins.item(self.store)) else {
            return false;
        };
        ctx.here().manhattan_distance(self.store) <= 1
            && ctx.think.features.has(store, self.store)
            && ctx.think.bins.stored(self.store) > 0
            && ctx.inventory.as_deref().is_some_and(|inventory| inventory.room_for(item))
    }
}

impl TaskExecutor for Fetch {
    /// **Checked every tick**, as [`Withdraw`](super::Withdraw) is: a unit
    /// pulled away, a store emptied meanwhile or a pack with no room ends the
    /// work there, and nothing is asked for.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if !self.can_fetch(ctx) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.store, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                *ctx.effect = Effect::Fetch { at: self.store };
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    /// Pulled away before it was taken: nothing came out, but the time spent
    /// at it was still work.
    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology)
            && *elapsed > 0.0
        {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("fetch one from the store at {}, {}", self.store.x, self.store.y)
    }
}

impl Task {
    /// Take one of what the store in `store` holds, from a cell beside it.
    pub const fn fetch(store: Point, seconds: f32) -> Task {
        Task::Fetch(Fetch { store, seconds })
    }
}

/// Saved as `fetch`: the store's cell and the seconds.
impl TimedTask for Fetch {
    const NAME: &'static str = "fetch";

    fn new(cell: Point, seconds: f32) -> Fetch {
        Fetch { store: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.store
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::item::ItemKind;

    const RACK: Point = Point::new(6, 6);

    fn rig_at(cell: Point, stored: u32) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "meal rack", RACK);
        let mut rig = Rig::new(map, cell);
        rig.biology.edit(|stats| stats.with_stamina(80.0));
        rig.world.bins = crate::sim::Bins::from_map(&rig.world.map);
        let _ = rig.world.bins.set(RACK, stored);
        rig
    }

    #[test]
    fn a_courier_fetches_one_meal_however_much_room_it_has() {
        let mut rig = rig_at(Point::new(5, 6), 4);
        let mut task = Task::fetch(RACK, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, Effect::None, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert_eq!(rig.effect, Effect::Fetch { at: RACK });
        let lines = rig.apply_effect();
        assert_eq!(rig.inventory.count(ItemKind::Meal), 1, "one, not a packful");
        assert_eq!(rig.world.bins.stored(RACK), 3);
        assert!(lines.iter().any(|line| line.contains("took 1 meal from the meal rack at 6, 6: 3 left")), "{lines:?}");
        assert!(rig.biology.stats().stamina() < 80.0, "fetching is work");
    }

    #[test]
    fn an_empty_rack_or_one_across_the_room_is_nothing_to_fetch() {
        assert_eq!(rig_at(Point::new(5, 6), 0).tick(&mut Task::fetch(RACK, 0.5)), TaskResult::Failed);
        let mut far = rig_at(Point::new(1, 1), 4);
        assert_eq!(far.tick(&mut Task::fetch(RACK, 0.5)), TaskResult::Failed);
        assert!(far.action.is_none());
    }
}
