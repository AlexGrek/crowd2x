//! Damaged distribution boxes. Switch position survives damage and repair;
//! damage independently disconnects the low-voltage output.

use crate::map::Point;

/// Testing rate. Change to 0.01 for normal play.
pub const BOX_FAILURE_CHANCE: f64 = 0.10;

#[derive(Default)]
pub struct Electrical {
    pub(crate) damaged: Vec<Point>,
    /// Repairs are settled without allocating in the world step. The next
    /// spawn pass rebuilds the supply and both daylight caches.
    pub(crate) supply_dirty: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, GridLayer, Map, Size, grid::power};
    use crate::sim::biology::ProcessId;
    use crate::sim::testing::prop_at;
    use crate::sim::{EntityType, GameState, Input, JobKind, process_game_state, spawn_pass};

    const BOX: Point = Point::new(3, 2);
    const FRIDGE: Point = Point::new(5, 2);

    fn map() -> Map {
        let mut map = Map::new(Size::new(8, 5), FLOOR);
        for (x, value) in [
            (1, power::LINE),
            (2, power::LINE),
            (3, power::BOX),
            (4, power::WIRING),
            (5, power::WIRING),
        ] {
            map.set_grid(GridLayer::Power, Point::new(x, 2), value);
        }
        prop_at(&mut map, "transformer", Point::new(1, 2));
        prop_at(&mut map, "fridge", FRIDGE);
        map
    }

    #[test]
    fn real_switches_can_fail_in_both_directions_but_noops_do_not_roll_again() {
        for on in [false, true] {
            let mut failures = 0;
            for seed in 0..1000 {
                let mut state = GameState::new(map(), seed);
                if on {
                    state.boxes_off.push(BOX);
                    state.refresh_supply();
                }
                let before = crate::sim::rng::state_of(&state.rng);
                state.switch_box(BOX, !on);
                state.switch_box(Point::new(0, 0), on);
                assert_eq!(
                    before,
                    crate::sim::rng::state_of(&state.rng),
                    "invalid or unchanged switches consume no randomness"
                );
                state.switch_box(BOX, on);
                if state.box_is_damaged(BOX) {
                    failures += 1;
                    assert!(!state.supply().serves("fridge", FRIDGE));
                    let before = crate::sim::rng::state_of(&state.rng);
                    state.switch_box(BOX, !on);
                    assert_eq!(state.box_is_on(BOX), on, "damage locks the switch");
                    assert_eq!(before, crate::sim::rng::state_of(&state.rng));
                }
            }
            assert!(
                (70..=130).contains(&failures),
                "{failures} of 1000 switches failed, expected about 10%"
            );
        }
    }

    #[test]
    fn an_electrician_walks_to_a_fault_repairs_it_and_preserves_the_switch_position() {
        for on in [false, true] {
            let mut state = GameState::new(map(), 42);
            let uid = state.spawn(EntityType::Human, Point::new(6, 3));
            assert_eq!(
                state.entities().get(uid).unwrap().job().unwrap().kind,
                JobKind::Electrician
            );
            let mut input = Input::new();
            for process in ProcessId::ALL {
                input.set_process(uid, process, false);
            }
            spawn_pass(&mut state, &input);
            if !on {
                state.boxes_off.push(BOX);
            }
            state.facilities.electrical.damage(BOX);
            state.refresh_supply();
            assert!(!state.supply().serves("fridge", FRIDGE));
            let mut repairing = false;
            for _ in 0..2400 {
                process_game_state(&mut state, 1.0 / 60.0, &Input::new());
                repairing |= state.entities().get(uid).unwrap().current_goal()
                    == Some(crate::sim::brain::GoalId::Repair);
                if !state.box_is_damaged(BOX) {
                    break;
                }
            }
            assert!(repairing);
            assert!(!state.box_is_damaged(BOX), "repair never finished");
            spawn_pass(&mut state, &Input::new());
            assert_eq!(state.box_is_on(BOX), on);
            assert_eq!(state.supply().serves("fridge", FRIDGE), on);
            assert!(
                state
                    .log
                    .drain()
                    .iter()
                    .any(|line| line.contains("repaired the distribution box"))
            );
        }
    }

    #[test]
    fn without_an_electrician_damage_does_not_repair_itself() {
        let mut state = GameState::new(map(), 42);
        state.facilities.electrical.damage(BOX);
        state.refresh_supply();
        for _ in 0..1200 {
            process_game_state(&mut state, 1.0 / 60.0, &Input::new());
        }
        assert!(state.box_is_damaged(BOX));
        assert!(!state.supply().serves("fridge", FRIDGE));
    }

    #[test]
    fn determinism_survives_electricians_repairing_in_the_parallel_rounds() {
        let build = || {
            let mut map = Map::new(Size::new(32, 24), FLOOR);
            map.set_grid(GridLayer::Power, BOX, power::BOX);
            let mut state = GameState::new(map, 42);
            let uid = state.spawn(EntityType::Human, Point::new(6, 3));
            let mut input = Input::new();
            for process in ProcessId::ALL {
                input.set_process(uid, process, false);
            }
            spawn_pass(&mut state, &input);
            for i in 0..(super::super::PARALLEL_AT as i32 + 20) {
                state.spawn(EntityType::Dog, Point::new(10 + i % 20, 8 + i / 20));
            }
            state.facilities.electrical.damage(BOX);
            state.refresh_supply();
            state
        };
        let (mut a, mut b) = (build(), build());
        let mut repairing = false;
        for _ in 0..1800 {
            process_game_state(&mut a, 1.0 / 60.0, &Input::new());
            process_game_state(&mut b, 1.0 / 60.0, &Input::new());
            repairing |= a
                .entities()
                .iter()
                .any(|e| e.current_goal() == Some(crate::sim::brain::GoalId::Repair));
        }
        assert!(repairing, "the new goal must actually run");
        assert!(!a.box_is_damaged(BOX) && !b.box_is_damaged(BOX));
        assert_eq!(a.save_world(), b.save_world());
        let positions = |state: &GameState| {
            state
                .entities()
                .iter()
                .map(|e| (e.uid(), e.position()))
                .collect::<Vec<_>>()
        };
        assert_eq!(positions(&a), positions(&b));
    }
}

impl Electrical {
    pub fn is_damaged(&self, at: Point) -> bool {
        self.damaged.binary_search(&at).is_ok()
    }

    pub fn damaged(&self) -> &[Point] {
        &self.damaged
    }

    pub(crate) fn damage(&mut self, at: Point) {
        if let Err(index) = self.damaged.binary_search(&at) {
            self.damaged.insert(index, at);
        }
    }

    pub(crate) fn repair(&mut self, at: Point) -> bool {
        let Ok(index) = self.damaged.binary_search(&at) else {
            return false;
        };
        self.damaged.remove(index);
        self.supply_dirty = true;
        true
    }
}
