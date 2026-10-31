use super::*;
use crate::map::{Map, Point, Size, FLOOR};
use economy::{Money, Tariff};
use testing::prop_at;

fn town(seed: u64) -> GameState {
    let mut map = Map::new(Size::new(20, 12), FLOOR);
    for (name, cell) in [("computer", Point::new(2, 2)), ("throne", Point::new(8, 2)),
        ("accountant computer", Point::new(14, 2))] {
        prop_at(&mut map, name, cell);
    }
    crate::map::utilities::serve_everything(&mut map, Point::new(0, 11), Point::new(19, 11));
    GameState::new(map, seed)
}

/// A known class for hiring tests; the quota tests use the real lottery.
fn arrive(state: &mut GameState, noble: bool, at: Point) -> Uid {
    state.noble_draws = 1;
    state.noble_position = if noble { 1 } else { 2 };
    state.spawn(EntityType::Human, at)
}

#[test]
fn every_ten_human_arrivals_include_exactly_one_randomly_selected_noble() {
    let mut positions = std::collections::BTreeSet::new();
    for seed in 0..32 {
        let mut state = GameState::new(Map::new(Size::new(4, 4), FLOOR), seed);
        for _ in 0..4 {
            let mut noble_positions = Vec::new();
            for index in 0..10 {
                state.spawn(EntityType::Dog, Point::new(1, 1));
                let uid = state.spawn(EntityType::Human, Point::new(1, 1));
                if state.entities.get(uid).unwrap().is_noble() { noble_positions.push(index); }
            }
            assert_eq!(noble_positions.len(), 1, "seed {seed}");
            positions.insert(noble_positions[0]);
        }
        assert_eq!(state.entities.iter().filter(|e| e.is_noble()).count(), 4);
    }
    assert_eq!(positions.len(), 10, "selection must not always pick the tenth arrival");
}

#[test]
fn nobles_take_only_noble_roles_and_replace_provisional_commoner_appointments() {
    let mut state = town(1);
    let clerk = arrive(&mut state, false, Point::new(3, 2));
    let king = arrive(&mut state, false, Point::new(9, 2));
    let accountant = arrive(&mut state, false, Point::new(15, 2));
    assert_eq!(state.entities.get(king).unwrap().job().unwrap().kind, JobKind::King);
    assert_eq!(state.entities.get(accountant).unwrap().job().unwrap().kind, JobKind::Accountant);
    let noble = arrive(&mut state, true, Point::new(3, 3));
    assert_eq!(state.entities.get(noble).unwrap().job().unwrap().kind, JobKind::King);
    assert!(state.entities.get(king).unwrap().job().is_none());
    assert!(state.entities.get(accountant).unwrap().job().is_none());
    assert!(!state.hire(accountant), "a living noble reserves the accountant role too");
    let second = arrive(&mut state, true, Point::new(3, 3));
    assert_eq!(state.entities.get(second).unwrap().job().unwrap().kind, JobKind::Accountant);
    state.dismiss(clerk);
    let third = arrive(&mut state, true, Point::new(3, 3));
    assert!(state.entities.get(third).unwrap().job().is_none(), "nobles cannot take the free ordinary desk");
    assert!(state.hire(clerk));
    assert_eq!(state.entities.get(clerk).unwrap().job().unwrap().kind, JobKind::OfficeWorker);
}

#[test]
fn a_noble_vacancy_prefers_a_farther_noble_and_falls_back_after_the_last_noble_dies() {
    let mut state = town(2);
    let clerk = arrive(&mut state, false, Point::new(3, 2));
    let king = arrive(&mut state, true, Point::new(9, 2));
    let accountant = arrive(&mut state, true, Point::new(15, 2));
    let heir = arrive(&mut state, true, Point::new(19, 10));
    let commoner = arrive(&mut state, false, Point::new(9, 3));
    let job = state.entities.get(king).unwrap().job().unwrap();
    state.entities.get_mut(king).unwrap().body_mut().set_dead();
    state.dismiss(king);
    assert!(state.fill_vacancy(king, job));
    assert_eq!(state.entities.get(heir).unwrap().job(), Some(job));
    state.entities.get_mut(heir).unwrap().body_mut().set_dead();
    state.dismiss(heir);
    assert!(!state.fill_vacancy(heir, job), "the living accountant still reserves noble roles");
    state.entities.get_mut(accountant).unwrap().body_mut().set_dead();
    state.dismiss(accountant);
    assert!(state.fill_vacancy(heir, job), "dead noble bodies must not prevent fallback");
    assert_eq!(state.entities.get(commoner).unwrap().job(), Some(job));
    assert_eq!(state.entities.get(clerk).unwrap().job().unwrap().kind, JobKind::OfficeWorker);
}

#[test]
fn payroll_pays_the_noble_care_rate_for_every_unemployed_day() {
    let mut state = GameState::new(Map::new(Size::new(4, 4), FLOOR), 3);
    let commoner = arrive(&mut state, false, Point::new(1, 1));
    let noble = arrive(&mut state, true, Point::new(2, 1));
    let economy = &mut state.facilities.economy;
    economy.set_tariff(Tariff::NobleCarePackage, 6500);
    let before = economy.budget();
    for uid in [commoner, noble] { economy.record(uid, 0, None, 2); }
    let mut payments = std::collections::BTreeMap::new();
    while let Some((uid, amount)) = economy.pay_next(commoner, &state.log) { payments.insert(uid, amount); }
    assert_eq!(payments[&commoner], Money::dollars(20));
    assert_eq!(payments[&noble], Money::dollars(130));
    assert_eq!(economy.budget(), before - Money::dollars(150));
    assert_eq!(economy.today().unwrap().care, Money::dollars(150));
}
