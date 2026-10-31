//! Economy through the real passes: shifts, payroll, shopping and reporting.
use super::*;
use super::economy::{Money, Tariff};
use crate::map::{Map, Size, FLOOR};
use super::testing::prop_at;

fn room(seed: u64) -> GameState {
    let mut map = Map::new(Size::new(18, 14), FLOOR);
    for (name, cell) in [("computer", Point::new(3, 9)), ("guard post", Point::new(8, 9)),
        ("accountant computer", Point::new(13, 9)), ("fridge", Point::new(13, 3))] {
        prop_at(&mut map, name, cell);
    }
    crate::map::utilities::serve_everything(&mut map, Point::new(0, 13), Point::new(17, 13));
    GameState::new(map, seed)
}

#[test]
fn a_working_day_pays_wages_and_care_and_the_accountant_reports_it() {
    let mut state = room(7);
    let mut people = Vec::new();
    let mut input = Input::new();
    for x in 2..8 {
        let uid = state.spawn(EntityType::Human, Point::new(x, 5));
        people.push(uid);
        for process in ProcessId::ALL { input.set_process(uid, process, false); }
    }
    let guard = people[1];
    let jobless = people[5];
    assert_eq!(state.entities.get(guard).unwrap().job().unwrap().kind, JobKind::Guard);
    assert_eq!(state.entities.get(people[4]).unwrap().job().unwrap().kind, JobKind::Accountant);
    assert!(state.entities.get(jobless).unwrap().job().is_none());
    let starting = state.economy().account(guard).unwrap().cash;
    let unemployed_cash = state.economy().account(jobless).unwrap().cash;
    spawn_pass(&mut state, &input);
    let dt = 1.0 / 60.0;
    // From Monday 08:00 through Tuesday 10:00, with needs disabled.
    for _ in 0..46_801 { process_pass(&mut state, dt); }
    assert_eq!(state.economy().account(guard).unwrap().cash, starting + Money::dollars(32));
    assert_eq!(state.economy().account(guard).unwrap().shifts, 0);
    assert_eq!(state.economy().account(jobless).unwrap().cash, unemployed_cash + Money::dollars(10));
    let book = state.economy().book(2).unwrap();
    assert!(book.wages >= Money::dollars(40));
    assert_eq!(book.taxes, book.wages.percent(20));
    assert_eq!(book.care, Money::dollars(10));
    assert_eq!(state.economy().budget(), Money::dollars(6000) + state.economy().books().iter().map(|book| book.balance()).sum::<Money>());
    assert_eq!(state.economy().closed_through(), 1);
    assert!(state.log.drain().iter().any(|line| line.contains("closed the books for day 1")));
}

#[test]
fn taking_food_from_a_fridge_charges_once_and_can_leave_a_human_in_debt() {
    let mut state = room(17);
    let uid = state.spawn(EntityType::Human, Point::new(12, 3));
    state.dismiss(uid);
    // A costly meal makes the charge and unlimited debt unambiguous.
    state.set_tariff(Tariff::Price(ItemKind::Food), 100_000);
    let before = state.economy().account(uid).unwrap().cash;
    let mut input = Input::new();
    for process in ProcessId::ALL {
        input.set_process(uid, process, process == ProcessId::Hunger);
    }
    spawn_pass(&mut state, &input);
    state.entities.get_mut(uid).unwrap().biology_mut().unwrap().edit(|stats| stats.with_hunger(70.0));
    for _ in 0..1500 { process_pass(&mut state, 1.0 / 60.0); }
    assert_eq!(state.economy().account(uid).unwrap().cash, before - Money::dollars(1000));
    assert_eq!(state.economy().today().unwrap().sales, Money::dollars(1000));
    assert!(state.economy().account(uid).unwrap().cash.is_negative());
}

#[test]
fn payroll_is_deterministic_in_the_parallel_rounds() {
    fn world() -> GameState {
        let mut state = room(31);
        state.elapsed = 1.5 * clock::HOUR as f64 / clock::TIME_SCALE as f64;
        let mut input = Input::new();
        for i in 0..80 {
            let uid = state.spawn(EntityType::Human, Point::new(1 + i % 16, 1 + i / 16));
            for process in ProcessId::ALL { input.set_process(uid, process, false); }
            state.facilities.economy.record(uid, 1, Some(JobKind::Guard), 0);
        }
        spawn_pass(&mut state, &input);
        state
    }
    let (mut a, mut b) = (world(), world());
    for _ in 0..400 {
        process_pass(&mut a, 1.0 / 60.0);
        process_pass(&mut b, 1.0 / 60.0);
        assert_eq!(a.economy(), b.economy());
    }
    assert!(a.economy().owing() < 80, "a clerk must actually have paid someone");
}

#[test]
fn the_district_bank_pays_and_the_palace_accountant_reports_the_first_day() {
    let district = crate::map::district::generate(7);
    let mut state = GameState::new(district.map, 7);
    state.spawn_from_spawners();
    let mut input = Input::new();
    for entity in state.entities().iter() {
        for process in ProcessId::ALL { input.set_process(entity.uid(), process, false); }
    }
    spawn_pass(&mut state, &input);
    for _ in 0..50_400 { process_pass(&mut state, 1.0 / 60.0); }
    assert!(state.economy().books().iter().any(|book| book.wages > Money::ZERO));
    assert_eq!(state.economy().closed_through(), 1);
    assert!(state.log.drain().iter().any(|line| line.contains("closed the books for day 1")));
}
