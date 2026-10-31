//! Shop, dispatch, purchasing and conservation through the real world passes.
use super::*;
use super::brain::GoalId;
use super::economy::Money;
use super::testing::{skip_to, store_world};

const FRIDGE: Point = Point::new(12, 5);
const RACK: Point = Point::new(2, 5);
const DT: f32 = 1.0 / 60.0;

#[test]
fn normalized_shop_balance_covers_startup_production_and_affordable_food() {
    use super::superfridge::StoreRules;
    let rules = StoreRules::default();
    let tariffs = super::economy::Tariffs::default();
    assert_eq!(rules.capacity, 1.0);
    assert_eq!(rules.delivery_size, 0.01);
    let servings_per_batch = rules.delivery_size / (rules.capacity * rules.falls_per_meal / 100.0);
    // No sleep discount: a conservative steady-state daily food allowance.
    let meals_per_day = 24.0 * 100.0 / super::biology::hunger::HOURS_TO_STARVING as f64 / ItemKind::Food.nutrition() as f64;
    assert_eq!(meals_per_day, 2.5);
    let net_wage = tariffs.wage(JobKind::StoreCourier) - tariffs.wage(JobKind::StoreCourier).percent(tariffs.tax_percent);
    // Care must cover a full waking day's food, with headroom for a pet
    // and meal timing; unemployment must not create ever-growing debt.
    let daily_food = tariffs.price(ItemKind::Food).as_cents() as f64 * meals_per_day;
    assert!(daily_food <= net_wage.as_cents() as f64, "a low wage feeds its earner");
    let generous_daily_food = tariffs.price(ItemKind::Food).as_cents() * 3
        + tariffs.price(ItemKind::DogFood).as_cents();
    assert!(generous_daily_food < tariffs.care_package.as_cents(),
        "even three meals and a pet dinner leave some care money over");
    assert_eq!(tariffs.price(ItemKind::Water), Money::ZERO, "drinking never adds debt");
    for seed in [7, 41, 97] {
        let district = crate::map::district::generate(seed);
        let mut state = GameState::new(district.map, seed);
        state.spawn_from_spawners();
        // Three servings for every resident, dogs included, leaves headroom
        // for pocket provisions and the initial hunger lottery.
        let daily_demand = state.len() as f64 * 3.0;
        let startup_batches = rules.initial() + state.bins().total_in(feature::FeatureKind::MealRack);
        assert!(startup_batches as f64 * servings_per_batch > daily_demand * super::fields::GROWING_DAYS as f64,
            "seed {seed}: starting food must bridge the first harvest");
        let weekly_batches = crate::map::district::food_factory::COOKERS as f64 * 8.0 * 5.0 / 0.5;
        assert!(weekly_batches * servings_per_batch > daily_demand * 7.0,
            "seed {seed}: factory production must cover a full week including weekends");
    }
}

fn run(state: &mut GameState, ticks: u32) {
    for _ in 0..ticks { process_pass(state, DT); }
}

#[test]
fn a_shopkeeper_calls_and_a_courier_brings_one_meal_without_charging_anybody() {
    let (mut state, people) = store_world(30, 10, 7);
    assert_eq!(state.entities().get(people[0]).unwrap().job().unwrap().kind, JobKind::Shopkeeper);
    assert_eq!(state.entities().get(people[4]).unwrap().job().unwrap().kind, JobKind::StoreCourier);
    let cash = state.economy().account(people[4]).unwrap().cash;
    run(&mut state, 3000);
    assert!(state.bins().stored(FRIDGE) > 30);
    assert_eq!(state.bins().stored(FRIDGE) + state.bins().stored(RACK), 40);
    assert!(state.dispatch().orders().is_empty());
    assert_eq!(state.economy().account(people[4]).unwrap().cash, cash);
    assert_eq!(state.economy().today().unwrap().sales, Money::ZERO);
    let lines = state.log.drain().join("\n");
    assert!(lines.contains("called the courier dispatch"), "{lines}");
    assert!(lines.contains("for the courier dispatch: 31% full"), "{lines}");
}

#[test]
fn an_empty_shop_resumes_feeding_fridges_after_delivery() {
    let (mut state, _) = store_world(0, 10, 7);
    assert!(!state.superfridge().feeds_fridges(state.bins()));
    for _ in 0..3000 {
        process_pass(&mut state, DT);
        if state.bins().stored(FRIDGE) > 0 { break; }
    }
    assert!(state.superfridge().feeds_fridges(state.bins()));
    assert!(state.log.drain().iter().any(|line| line.contains("the superfridge has food again")));
}

#[test]
fn repeated_checks_without_food_or_a_courier_keep_one_waiting_call() {
    for meals in [0, 10] {
        let (mut state, people) = store_world(10, meals, 7);
        if meals > 0 { state.dismiss(people[4]); }
        run(&mut state, 3000);
        assert_eq!(state.dispatch().waiting(), 1);
        assert_eq!(state.dispatch().orders().len(), 1);
        assert_eq!(state.bins().stored(FRIDGE), 10);
    }
}

#[test]
fn a_delivery_full_on_arrival_keeps_the_excess_and_returns_it_to_the_warehouse() {
    let (mut state, people) = store_world(30, 10, 7);
    let courier = people[4];
    for _ in 0..3000 {
        process_pass(&mut state, DT);
        if state.entities().get(courier).unwrap().inventory().unwrap().count(ItemKind::Meal) == 1 { break; }
    }
    assert_eq!(state.entities().get(courier).unwrap().inventory().unwrap().count(ItemKind::Meal), 1);
    assert!(state.facilities.bins.set(FRIDGE, 100));
    run(&mut state, 3000);
    assert_eq!(state.bins().stored(FRIDGE), 100);
    assert_eq!(state.bins().stored(RACK), 10, "the undeliverable meal returns");
    assert!(state.dispatch().orders().is_empty());
}

#[test]
fn dispatch_work_clocks_in_and_pays_the_couriers_named_wage() {
    let (mut state, people) = store_world(30, 10, 7);
    let courier = people[4];
    // Assigned before its first brain tick, so it never clocks in via Haul.
    state.facilities.dispatch.call(FRIDGE);
    let now = state.clock();
    let entities = &state.entities;
    state.facilities.dispatch.send(entities.iter(), |uid| entities.get(uid), &state.facilities.bins, &state.facilities.superfridge, now, &state.log);
    assert!(state.dispatch().order_of(courier).is_some());
    process_pass(&mut state, DT);
    assert_eq!(state.entities().get(courier).unwrap().current_goal(), Some(GoalId::Deliver));
    skip_to(&mut state, 1, 16.1);
    run(&mut state, 3);
    assert_eq!(state.economy().account(courier).unwrap().shifts, 1);
    let cash = state.economy().account(courier).unwrap().cash;
    state.facilities.economy.pay_next(people[0], &state.log);
    // Payroll is round-robin; pay through the other keepers if needed.
    for _ in 0..5 { state.facilities.economy.pay_next(people[0], &state.log); }
    assert_eq!(state.economy().account(courier).unwrap().cash, cash + Money::dollars(32));
}

#[test]
fn shop_and_delivery_remain_deterministic_in_parallel_rounds() {
    let (mut a, _) = store_world(30, 10, 7);
    let (mut b, _) = store_world(30, 10, 7);
    for state in [&mut a, &mut b] {
        for _ in 0..65 {
            let uid = state.spawn_commoner(Point::new(16, 12));
            state.entities.get_mut(uid).unwrap().body_mut().set_frozen(true);
        }
    }
    for _ in 0..2500 {
        process_pass(&mut a, DT);
        process_pass(&mut b, DT);
        assert_eq!(a.facilities.dispatch, b.facilities.dispatch);
        assert_eq!(a.facilities.bins, b.facilities.bins);
        assert_eq!(a.economy(), b.economy());
        for (left, right) in a.entities().iter().zip(b.entities().iter()) {
            assert_eq!(left.position(), right.position());
            assert_eq!(left.current_goal(), right.current_goal());
        }
    }
    assert!(a.bins().stored(FRIDGE) > 30, "delivery actually ran");
}

#[test]
fn purchases_finishing_together_charge_only_the_meal_backed_by_stock() {
    let (mut state, people) = store_world(1, 0, 7);
    let mut rules = state.superfridge().rules();
    rules.falls_per_meal = 100.0;
    state.set_store_rules(rules);
    let before = state.economy().account(people[1]).unwrap().cash;
    for uid in [people[0], people[1]] {
        let entity = state.entities.get_mut(uid).unwrap();
        let _ = entity.inventory_mut().unwrap().set_hand(Some(ItemKind::Food));
        effect::apply(Effect::Buy { item: ItemKind::Food }, entity, &mut state.map, &mut state.facilities, &mut state.occupancy, &state.log);
    }
    assert_eq!(state.entities().get(people[0]).unwrap().inventory().unwrap().hand(), Some(ItemKind::Food));
    assert_eq!(state.entities().get(people[1]).unwrap().inventory().unwrap().hand(), None);
    assert_eq!(state.economy().account(people[1]).unwrap().cash, before);
    assert_eq!(state.economy().today().unwrap().sales, state.economy().tariffs().price(ItemKind::Food));
    assert_eq!(state.bins().stored(FRIDGE), 0);
}

#[test]
fn a_deposit_exceeding_capacity_returns_every_unaccepted_meal() {
    let (mut state, people) = store_world(99, 0, 7);
    let courier = state.entities.get_mut(people[4]).unwrap();
    // These two pieces have just left its pack during the deposit task.
    effect::apply(Effect::Deposit { at: FRIDGE, pieces: 2 }, courier, &mut state.map, &mut state.facilities, &mut state.occupancy, &state.log);
    assert_eq!(state.bins().stored(FRIDGE), 100);
    assert_eq!(state.entities().get(people[4]).unwrap().inventory().unwrap().count(ItemKind::Meal), 1);
}

#[test]
fn a_need_interrupts_a_delivery_without_losing_its_order_or_food() {
    let (mut state, people) = store_world(30, 10, 7);
    let uid = people[4];
    for _ in 0..2500 {
        process_pass(&mut state, DT);
        if state.entities().get(uid).unwrap().inventory().unwrap().count(ItemKind::Meal) == 1 { break; }
    }
    assert!(state.dispatch().order_of(uid).is_some());
    let biology = state.entities.get_mut(uid).unwrap().biology_mut().unwrap();
    biology.edit(|stats| stats.with_bladder(95.0));
    biology.set_running(ProcessId::Bladder, true);
    process_pass(&mut state, DT);
    assert_ne!(state.entities().get(uid).unwrap().current_goal(), Some(GoalId::Deliver));
    assert_eq!(state.entities().get(uid).unwrap().inventory().unwrap().count(ItemKind::Meal), 1);
    assert!(state.dispatch().order_of(uid).is_some());
    state.entities.get_mut(uid).unwrap().biology_mut().unwrap().set_running(ProcessId::Bladder, false);
    run(&mut state, 3000);
    assert!(state.bins().stored(FRIDGE) > 30);
    assert!(state.dispatch().orders().is_empty());
}
