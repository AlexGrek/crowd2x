//! Deathdogs, panic and panic by sight, through the real passes and through
//! a world of one.

use rand::SeedableRng;

use super::*;
use crate::map::{Size, FLOOR};
use crate::sim::biology::ProcessId;
use crate::sim::brain::{GoalId, Spotted, Task};
use crate::sim::clock::{Clock, HOUR, MINUTE, TIME_SCALE};
use crate::sim::entity::GameEntity;
use crate::sim::kinds::{Deathdog, Facing, DEATHDOG_BITES, DEATHDOG_SPEED};
use crate::sim::kinds::Dog;
use crate::sim::refuge::{depth, SAFE_DEPTH};
use crate::sim::testing::{needy_human, skip_to, World};
use crate::sim::uid::Uid;
use crate::sim::walker::{Walker, HURRY, PANIC_SPEED, ROUTES_ASKED};
use crate::sim::wounds::Wound;
use crate::sim::{process_game_state, process_pass, spawn_pass, Input, MoveOutcome};

const DT: f32 = 1.0 / 60.0;

#[test]
fn a_dog_takes_bite_damage_spills_blood_and_heals_without_hospital_care() {
    let mut world = World::new(endless(Size::new(10, 10), 5));
    let mut dog = Dog::new(Uid::new(EntityType::Dog, 31), Point::new(-3, 2), Facing::Right, &mut rand::rngs::SmallRng::seed_from_u64(1));
    let wound = |damage| Wound { target: dog.uid(), by: Uid::new(EntityType::Deathdog, 32), from: Point::new(-4, 2), damage };
    world.wounds.inflict(wound(30));
    world.step(&mut dog);
    let health = dog.biology().unwrap().stats().health();
    assert!((health - 70.0).abs() < 0.01, "{health}");
    assert_eq!(dog.body().dirt().blood, 0.3);
    assert_eq!(world.dirt.at(dog.center_position()).blood, 0.3);
    assert!(!dog.is_dead());
    world.dt = HOUR / TIME_SCALE;
    world.step(&mut dog);
    assert!((dog.biology().unwrap().stats().health() - health - 100.0 / 24.0).abs() < 0.01);
    world.dt *= 24.0;
    world.step(&mut dog);
    assert_eq!(dog.biology().unwrap().stats().health(), 100.0);
}

#[test]
fn switching_off_a_dogs_health_stops_both_damage_and_healing_and_blood() {
    let mut world = World::new(endless(Size::new(10, 10), 5));
    let mut dog = Dog::new(Uid::new(EntityType::Dog, 35), Point::new(-3, 2), Facing::Right, &mut rand::rngs::SmallRng::seed_from_u64(1));
    let biology = dog.biology_mut().unwrap();
    biology.edit(|s| s.with_health(60.0));
    biology.set_running(ProcessId::Health, false);
    world.wounds.inflict(Wound { target: dog.uid(), by: Uid::new(EntityType::Deathdog, 32), from: Point::new(-4, 2), damage: 30 });
    world.step(&mut dog);
    world.dt = HOUR / TIME_SCALE;
    world.step(&mut dog);
    assert_eq!(dog.biology().unwrap().stats().health(), 60.0);
    assert_eq!(dog.body().dirt().blood, 0.0);
}

#[test]
fn a_fatal_bite_kills_a_dog_leaves_blood_and_releases_its_cell() {
    let mut state = GameState::new(endless(Size::new(10, 10), 5), 5);
    let dog = state.spawn(EntityType::Dog, Point::new(-3, 2));
    state.entity_mut(dog).unwrap().biology_mut().unwrap().edit(|s| s.with_health(20.0));
    state.facilities.wounds.inflict(Wound { target: dog, by: Uid::new(EntityType::Deathdog, 32), from: Point::new(-4, 2), damage: 30 });
    process_pass(&mut state, DT);
    let dead = state.entities().get(dog).unwrap();
    assert!(dead.is_dead());
    assert_eq!(dead.biology().unwrap().stats().health(), 0.0);
    assert_eq!(dead.body().dirt().blood, 0.3);
    assert_eq!(state.dirt().at(dead.center_position()).blood, 0.3);
    assert_eq!(state.occupancy().occupant(dead.center_position()), None);
    assert!(state.log.drain().iter().any(|line| line.contains("died of deathdog bites")));
}

#[test]
fn a_deathdog_hunts_and_bites_a_regular_dog_outside_but_not_inside() {
    for outside in [true, false] {
        let mut world = World::new(endless(Size::new(12, 12), 8));
        world.clock = at(23.0);
        let prey = Uid::new(EntityType::Dog, 33);
        let cell = if outside { Point::new(-3, 3) } else { Point::new(0, 3) };
        world.occupancy.claim(cell, prey).unwrap();
        let mut hunter = Deathdog::new(Uid::new(EntityType::Deathdog, 34), cell.offset(0, 3), Facing::Right, &mut rand::rngs::SmallRng::seed_from_u64(1));
        hunter.brain_mut().memory_mut().spot(Spotted { who: prey, at: cell, when: world.clock.elapsed() });
        let mut bitten = false;
        for _ in 0..300 {
            world.step(&mut hunter);
            if world.wounds.on(prey).next().is_some() {
                bitten = true;
                break;
            }
        }
        assert_eq!(bitten, outside, "outside: {outside}, {:?}", world.lines());
    }
}

/// An open site of `size` floor with the wasteland of `seed` round it.
fn endless(size: Size, seed: u64) -> crate::map::Map {
    Map::new(size, FLOOR).with_wasteland(seed)
}

fn deathdogs(state: &GameState) -> Vec<Uid> {
    state.entities().iter().filter(|e| e.kind() == Some(EntityType::Deathdog)).map(|e| e.uid()).collect()
}

fn run(state: &mut GameState, ticks: u32) {
    let input = Input::new();
    for _ in 0..ticks {
        process_game_state(state, DT, &input);
    }
}

/// From 19:00 on day 1, still light, tick until it has got dark.
fn until_dark(state: &mut GameState) {
    skip_to(state, 1, 19.0);
    let mut ticks = 0;
    while !state.clock().sun().is_dark() {
        run(state, 1);
        ticks += 1;
        assert!(ticks < 5000, "it never got dark");
    }
}

/// `hours` o'clock on day 1, as a clock a [`World`] can be set to.
fn at(hours: f32) -> Clock {
    Clock::after_watching((hours as f64 - 8.0) * HOUR as f64 / TIME_SCALE as f64)
}

#[test]
fn two_packs_of_three_come_out_at_dusk_far_out_and_only_then() {
    let mut state = GameState::new(endless(Size::new(16, 16), 3), 3);
    run(&mut state, 600);
    assert!(deathdogs(&state).is_empty(), "none by day");

    until_dark(&mut state);
    let pack = deathdogs(&state);
    assert_eq!(pack.len(), PACKS * PACK_SIZE);
    for uid in &pack {
        let cell = state.entities().get(*uid).unwrap().center_position();
        assert!(distance_from_site(&state.map, cell) >= SPAWN_DISTANCE, "{uid} came out at {cell:?}");
    }
    let lines = state.log.drain();
    assert_eq!(lines.iter().filter(|line| line.contains("a pack of 3 deathdogs came out of the wasteland")).count(), PACKS, "{lines:?}");
    run(&mut state, 60);
    assert_eq!(deathdogs(&state).len(), PACKS * PACK_SIZE, "once a night");

    let mut bounded = GameState::new(Map::new(Size::new(16, 16), FLOOR), 3);
    until_dark(&mut bounded);
    assert!(deathdogs(&bounded).is_empty(), "no wasteland, no deathdogs");
}

#[test]
fn a_deathdog_routes_round_the_site_and_is_refused_a_step_onto_it() {
    let world = World::new(endless(Size::new(10, 10), 4));
    let (from, to) = (Point::new(-3, 5), Point::new(13, 5));
    let path = |kind: EntityType| {
        let mut walker = Walker::new(Uid::new(kind, 1), from, 4.0);
        assert!(walker.route_to(&world.ctx(), to));
        walker.path_cells()
    };
    assert!(path(EntityType::Human).iter().any(|&cell| world.map.site().contains(cell)), "a person cuts across");
    let round = path(EntityType::Deathdog);
    assert!(!round.iter().any(|&cell| world.map.site().contains(cell)), "{round:?}");

    let mut state = GameState::new(endless(Size::new(10, 10), 4), 4);
    skip_to(&mut state, 1, 23.0);
    let dog = state.spawn(EntityType::Deathdog, Point::new(-1, 5));
    let entity = state.entity_mut(dog).unwrap();
    entity.body_mut().set_position((-0.01, 5.5));
    entity.walker_mut().unwrap().set_path(vec![Point::new(0, 5)]);
    run(&mut state, 1);
    assert_eq!(state.last_move(dog), Some(MoveOutcome::Blocked { by: None }));
    assert!(state.position_of(dog).unwrap().0 < 0.0);

    let mut input = Input::new();
    input.spawn(EntityType::Deathdog, Point::new(5, 5));
    process_game_state(&mut state, DT, &input);
    assert_eq!(deathdogs(&state), [dog], "none spawned on the site");
    assert!(state.log.drain().iter().any(|line| line.contains("keeps off the site")));
}

/// A person outside, one bite at a time: it hurts as much as it says, it
/// frightens, and enough of it kills — but not a body that cannot be hurt.
#[test]
fn a_bite_takes_its_damage_frightens_and_enough_of_them_kill() {
    let mut world = World::new(endless(Size::new(10, 10), 5));
    world.clock = at(23.0);
    let mut human = needy_human(Point::new(-6, 6), 0.0, 0.0, 0.0);
    let dog = Uid::new(EntityType::Deathdog, 9);
    let target = human.uid();
    let wound = |damage| Wound { target, by: dog, from: Point::new(-7, 6), damage };
    let before = human.stats().health();
    world.wounds.inflict(wound(30));
    world.step(&mut human);
    assert!((before - 30.0 - human.stats().health()).abs() < 0.1, "{before} -> {}", human.stats().health());
    assert_eq!(human.brain().top_goal(), GoalId::Flee);
    assert_eq!(human.brain().memory().spotted().map(|spotted| spotted.who), Some(dog));
    assert!(world.log_contains("panicked"));

    human.biology_mut().unwrap().set_running(ProcessId::Health, false);
    let shrugged = human.stats().health();
    world.wounds.inflict(wound(60));
    world.step(&mut human);
    assert!((human.stats().health() - shrugged).abs() < 0.1, "health switched off: a bite does nothing");

    human.biology_mut().unwrap().set_running(ProcessId::Health, true);
    human.biology_mut().unwrap().edit(|stats| stats.with_health(10.0));
    world.wounds.inflict(wound(40));
    world.step(&mut human);
    assert!(human.is_dead());
    assert!(world.log_contains(&format!("{} died of {DEATHDOG_BITES}", human.uid())), "{:?}", world.lines());
    assert_eq!(world.corpses.at(human.center_position()), Some(human.uid()));
}

/// Somebody outside who sees a deathdog panics: runs flat out — exactly as
/// fast as a hunting deathdog — to the nearest way in, shelters inside, and
/// carries on with its day once the fright is over.
#[test]
fn a_person_who_sees_a_deathdog_runs_in_flat_out_shelters_and_calms_down() {
    assert_eq!(PANIC_SPEED, DEATHDOG_SPEED * (1.0 + HURRY), "a hunting deathdog is exactly as fast");
    let mut world = World::new(endless(Size::new(12, 12), 6));
    world.clock = at(23.0);
    let mut human = needy_human(Point::new(-8, 6), 0.0, 0.0, 0.0);
    // South of it, three cells: in front of somebody who has never moved.
    let _ = world.occupancy.claim(Point::new(-8, 3), Uid::new(EntityType::Deathdog, 11));
    let mut ran_flat_out = false;
    let mut sheltered = false;
    for _ in 0..1200 {
        world.step(&mut human);
        let walker = human.walker().unwrap();
        if walker.is_walking() && human.brain().top_goal() == GoalId::Flee {
            assert!(human.body().is_panicking());
            assert_eq!(walker.current_speed(), PANIC_SPEED);
            ran_flat_out = true;
        }
        if depth(&world.map, human.center_position()) >= SAFE_DEPTH && matches!(human.brain().current_task(), Some(Task::Wait(_))) {
            sheltered = true;
            break;
        }
    }
    assert!(world.log_contains("panicked"));
    assert!(ran_flat_out, "never ran");
    assert!(sheltered, "never got in: at {:?}", human.center_position());
    assert!(!human.body().is_panicking(), "standing still is not panicking");

    world.clock = Clock::after_watching(world.clock.elapsed() / TIME_SCALE as f64 + (31.0 * MINUTE / TIME_SCALE) as f64);
    for _ in 0..4 {
        world.step(&mut human);
    }
    assert_ne!(human.brain().top_goal(), GoalId::Flee, "the fright is over");
}

/// Panic spreads by sight: somebody outside who sees only a person running
/// in a panic panics too. Somebody inside sees a deathdog and stays calm.
#[test]
fn panic_spreads_to_whoever_outside_sees_it_and_nobody_inside_reacts() {
    let looked = |observer: Point, seen: Uid, seen_at: Point, panicking: bool| {
        let mut world = World::new(endless(Size::new(12, 12), 7));
        world.clock = at(23.0);
        let mut human = needy_human(observer, 0.0, 0.0, 0.0);
        let _ = world.occupancy.claim(seen_at, seen);
        world.panics.set(seen, panicking);
        for _ in 0..20 {
            world.step(&mut human);
        }
        human.brain().top_goal() == GoalId::Flee
    };
    let runner = Uid::new(EntityType::Human, 12);
    assert!(looked(Point::new(-8, 6), runner, Point::new(-8, 3), true), "caught it from somebody panicking");
    assert!(!looked(Point::new(-8, 6), runner, Point::new(-8, 3), false), "somebody calm is nothing to run from");
    let dog = Uid::new(EntityType::Deathdog, 13);
    assert!(looked(Point::new(-8, 6), dog, Point::new(-8, 3), false), "a deathdog is");
    assert!(!looked(Point::new(6, 2), dog, Point::new(6, -1), false), "inside is safe");
}

/// A deathdog at night with somebody spotted outside.
fn hunter(world: &World, quarry: Uid, at: Point) -> Deathdog {
    let mut dog = Deathdog::new(Uid::new(EntityType::Deathdog, 21), Point::new(-24, 6), Facing::Left, &mut rand::rngs::SmallRng::seed_from_u64(1));
    dog.brain_mut().memory_mut().spot(Spotted { who: quarry, at, when: world.clock.elapsed() });
    dog
}

#[test]
fn a_deathdog_runs_down_somebody_outside_bites_and_gives_up_once_they_are_inside() {
    let mut world = World::new(endless(Size::new(12, 12), 8));
    world.clock = at(23.0);
    let victim = Uid::new(EntityType::Human, 22);
    let _ = world.occupancy.claim(Point::new(-12, 6), victim);
    let mut dog = hunter(&world, victim, Point::new(-12, 6));
    let mut bitten = false;
    for _ in 0..600 {
        world.step(&mut dog);
        if world.wounds.on(victim).next().is_some() {
            bitten = true;
            break;
        }
    }
    assert!(bitten, "{:?}", world.lines());
    assert!(world.log_contains(&format!("bit {victim} for")));
    let damage = world.wounds.on(victim).next().unwrap().damage;
    assert!(BITE_DAMAGE.contains(&damage));

    world.occupancy.release(Point::new(-12, 6), victim);
    let _ = world.occupancy.claim(Point::new(1, 6), victim);
    for _ in 0..30 {
        world.step(&mut dog);
    }
    assert_eq!(dog.brain().memory().spotted(), None, "lost to the site");
    assert_eq!(dog.brain().top_goal(), GoalId::Prowl);
    assert!(world.wounds.iter().all(|wound| wound.target != victim));
}

/// A quarry running away exactly as fast as the deathdog is never caught,
/// and the chase asks for a route only every few cells, not every tick.
#[test]
fn a_chase_that_cannot_be_won_asks_for_few_routes() {
    let mut world = World::new(endless(Size::new(12, 12), 9));
    world.clock = at(23.0);
    let victim = Uid::new(EntityType::Human, 23);
    let mut cell = Point::new(-30, 6);
    let _ = world.occupancy.claim(cell, victim);
    let mut dog = hunter(&world, victim, cell);
    ROUTES_ASKED.with(|asked| asked.set(0));
    let ticks = 640;
    let per_cell = 1.0 / (PANIC_SPEED * world.dt);
    for tick in 0..ticks {
        if (tick as f32 / per_cell).floor() > ((tick as f32 - 1.0) / per_cell).floor() {
            world.occupancy.release(cell, victim);
            cell = cell.offset(-1, 0);
            let _ = world.occupancy.claim(cell, victim);
        }
        world.step(&mut dog);
    }
    let asked = ROUTES_ASKED.with(|asked| asked.get());
    assert!(asked < ticks / 8, "{asked} routes in {ticks} ticks");
    assert_eq!(dog.brain().top_goal(), GoalId::Hunt, "still after it");
}

/// By day a deathdog walks off and leaves the world past
/// [`DEPART_DISTANCE`] — driven the way the game drives a world, the spawn
/// pass and the processing pass apart, which is where leaving happens now.
/// A body taken up leaves the same way.
#[test]
fn by_day_a_deathdog_walks_off_and_leaves_driven_as_the_game_drives_it() {
    let mut state = GameState::new(endless(Size::new(10, 10), 10), 10);
    let dog = state.spawn(EntityType::Deathdog, Point::new(-20, 5));
    let input = Input::new();
    let mut ticks = 0;
    while state.entities().get(dog).is_some() {
        spawn_pass(&mut state, &input);
        process_pass(&mut state, DT);
        ticks += 1;
        assert!(ticks < 6000, "still here, at {:?}", state.position_of(dog));
    }
    assert!(state.log.drain().iter().any(|line| line.contains("went back into the wasteland")));

    let human = state.spawn(EntityType::Human, Point::new(5, 5));
    state.facilities.leaving.push(human);
    spawn_pass(&mut state, &input);
    process_pass(&mut state, DT);
    assert!(state.entities().get(human).is_none(), "a body taken up leaves with the two passes alone");
}

/// Nobody inside is ever bitten: a crowd wandering a site at night, with
/// deathdogs prowling round it.
#[test]
fn nobody_inside_the_site_is_ever_bitten() {
    // Walled all round, so nobody wanders out.
    let mut map = endless(Size::new(14, 14), 11);
    for cell in map.site().points().collect::<Vec<_>>() {
        if map.site().on_edge(cell) {
            map.set_terrain(cell, crate::map::WALL);
        }
    }
    let mut state = GameState::new(map, 11);
    for i in 0..6 {
        let uid = state.spawn(EntityType::Human, Point::new(2 + i * 2, 1 + i % 3));
        let mut input = Input::new();
        for process in ProcessId::ALL {
            input.set_process(uid, process, false);
        }
        process_game_state(&mut state, DT, &input);
    }
    until_dark(&mut state);
    for _ in 0..2400 {
        run(&mut state, 1);
        assert!(state.facilities.wounds.is_empty(), "somebody inside was bitten");
    }
    assert!(!state.log.drain().iter().any(|line| line.contains(" bit ")));
}

/// The same seed, the same night: the packs come out in the same places, and
/// the hunt and the flight go the same way, to the bit.
#[test]
fn a_night_of_deathdogs_comes_out_the_same_every_time() {
    let night = || {
        let mut state = GameState::new(endless(Size::new(12, 12), 12), 12);
        for i in 0..8 {
            state.spawn(EntityType::Human, Point::new(-6 - i % 4, 2 + i));
        }
        until_dark(&mut state);
        run(&mut state, 1500);
        let mut seen: Vec<(Uid, u32, u32)> = state
            .entities()
            .iter()
            .map(|entity| (entity.uid(), entity.position().0.to_bits(), entity.position().1.to_bits()))
            .collect();
        seen.sort();
        seen
    };
    let first = night();
    assert!(first.iter().filter(|(uid, ..)| uid.kind() == Some(EntityType::Deathdog)).count() > 0);
    assert_eq!(first, night());
}


#[test]
fn the_ring_is_walked_round_every_cell_once() {
    let origin = Point::new(-2, -3);
    let (width, height) = (5, 4);
    let cells: Vec<Point> = (0..2 * (width + height) - 4).map(|along| on_ring(origin, width, height, along)).collect();
    let mut unique = cells.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), cells.len());
    let bounds = crate::map::Bounds::new(origin, Size::new(width, height));
    assert!(cells.iter().all(|&cell| bounds.on_edge(cell)), "{cells:?}");
}
