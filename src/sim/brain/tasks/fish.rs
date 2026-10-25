//! One cast from a dry bank. Only a completed attempt can earn a fish.

use rand::RngExt;
use crate::map::{Point, WATER};
use crate::sim::biology::{Biology, Event};
use crate::sim::clock::{watched, MINUTE, TIME_SCALE};
use crate::sim::item::ItemKind;
use crate::sim::rng::tick_rng;
use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// A cast takes ten world minutes and has a one-in-two chance of a catch.
pub const FISH_SECONDS: f32 = watched(10.0 * MINUTE);
pub const CATCH_CHANCE: f64 = 0.5;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fish {
    pub water: Point,
    pub seconds: f32,
}

impl TaskExecutor for Fish {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.water) != 1
            || ctx.think.map.contains(self.water)
            || ctx.think.map.terrain(self.water) != Some(WATER)
            || !ctx.think.is_passable_with(ctx.here(), ctx.walk.body().home())
            || !ctx.inventory.as_deref().is_some_and(|pack| pack.count(ItemKind::Fish) < 5 && pack.room_for(ItemKind::Fish))
        {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.water, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked { world_seconds: self.seconds * TIME_SCALE });
                }
                let uid = ctx.walk.body().uid();
                if tick_rng(uid, ctx.think.tick).random_bool(CATCH_CHANCE) {
                    let pack = ctx.inventory.as_deref_mut().unwrap();
                    if pack.stow(ItemKind::Fish) {
                        ctx.think.log.unit(uid, format!("{uid} caught a fish at {}, {}: {} carried", self.water.x, self.water.y, pack.count(ItemKind::Fish)));
                    }
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn interrupted(&mut self, action: &Action, biology: Option<&mut Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology) {
            biology.handle(Event::Worked { world_seconds: elapsed * TIME_SCALE });
        }
    }

    fn describe(&self) -> String {
        format!("fish at {}, {}", self.water.x, self.water.y)
    }
}

impl Task {
    pub const fn fish(water: Point, seconds: f32) -> Task {
        Task::Fish(Fish { water, seconds })
    }
}

impl TimedTask for Fish {
    const NAME: &'static str = "fish";
    fn new(water: Point, seconds: f32) -> Fish { Fish { water, seconds } }
    fn cell(&self) -> Point { self.water }
    fn seconds(&self) -> f32 { self.seconds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::brain::tasks::rig::Rig;

    fn bank() -> (Rig, Point) {
        let mut map = Map::new(Size::new(9, 7), FLOOR).with_wasteland(0);
        map.load_chunks(map.navigation_area());
        let (water, shore) = map.navigation_area().points().filter(|&p| !map.contains(p) && map.terrain(p) == Some(WATER))
            .find_map(|p| p.cardinal_neighbours().into_iter().find(|&side| map.is_passable(side)).map(|side| (p, side))).unwrap();
        (Rig::new(map, shore), water)
    }

    #[test]
    fn completed_casts_can_catch_or_miss_and_never_exceed_five_fish() {
        let (mut rig, water) = bank();
        let mut caught = 0;
        let mut missed = 0;
        for _ in 0..100 {
            rig.action = Action::None;
            let mut task = Task::fish(water, 0.0);
            assert_eq!(rig.tick(&mut task), TaskResult::Executing);
            assert_eq!(rig.inventory.count(ItemKind::Fish), 0, "nothing before completion");
            assert_eq!(rig.tick(&mut task), TaskResult::Success);
            if rig.inventory.take_out(ItemKind::Fish) { caught += 1; } else { missed += 1; }
        }
        assert!(caught > 0 && missed > 0, "{caught} catches, {missed} misses");
        for _ in 0..5 { assert!(rig.inventory.stow(ItemKind::Fish)); }
        assert!(!rig.inventory.stow(ItemKind::Fish));
        rig.action = Action::None;
        assert_eq!(rig.tick(&mut Task::fish(water, 0.0)), TaskResult::Failed);
    }

    #[test]
    fn a_cast_abandoned_before_completion_yields_nothing_and_remembers_the_work() {
        let (mut rig, water) = bank();
        let mut task = Task::fish(water, FISH_SECONDS);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        for _ in 0..20 { assert_eq!(rig.tick(&mut task), TaskResult::Executing); }
        let before = rig.biology.stats().stamina();
        task.interrupted(&rig.action, Some(&mut rig.biology));
        assert_eq!(rig.inventory.count(ItemKind::Fish), 0);
        assert!(rig.biology.stats().stamina() < before);
        // Lose the dry bank midway: fail without a catch.
        rig.world.map = Map::new(Size::new(9, 7), FLOOR);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.inventory.count(ItemKind::Fish), 0);
    }
}
