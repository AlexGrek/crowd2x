use super::*;
use crate::map::{FLOOR, Map, Point, Size, WALL};
use crate::sim::biology::bladder::CRITICAL;
use crate::sim::testing::{World, needy_human, prop_at};
use crate::sim::uid::{EntityType, Uid};
use crate::sim::wounds::Wound;
use crate::sim::{GameState, Input, process_game_state};

#[test]
fn hourly_evaporation_drains_only_water_in_all_cells_even_in_distant_chunks() {
    let map = Map::new(Size::new(4, 4), FLOOR);
    let mut layer = crate::sim::dirt::DirtLayer::new(&map);
    let stain = Dirt {
        dirt: 0.5,
        blood: 0.4,
        wet: 1.0,
        burns: 0.3,
    };
    let cells = [Point::new(0, 0), Point::new(3, 3), Point::new(-5000, 7000)];
    for cell in cells {
        layer.set(cell, stain);
    }
    layer.evaporate(0);
    assert_eq!(layer.at(cells[0]), stain);
    for hour in 1..=4 {
        layer.evaporate(1);
        for cell in cells {
            assert_eq!(
                layer.at(cell),
                Dirt {
                    wet: 1.0 - hour as f32 * 0.25,
                    ..stain
                }
            );
        }
    }
    layer.evaporate(100);
    for cell in cells {
        assert_eq!(layer.at(cell), Dirt { wet: 0.0, ..stain });
    }
}

#[test]
fn evaporation_runs_once_per_world_hour_and_catches_up_multiple_hours() {
    use crate::sim::clock::{HOUR, TIME_SCALE};
    let mut state = GameState::new(Map::new(Size::new(1, 1), FLOOR), 3);
    let cell = Point::new(0, 0);
    state.facilities.dirt.set(cell, PEE);
    state.elapsed = (HOUR / TIME_SCALE - 0.5) as f64;
    crate::sim::process_pass(&mut state, 0.25);
    assert_eq!(state.dirt().at(cell).wet, 1.0);
    crate::sim::process_pass(&mut state, 0.25);
    assert_eq!(state.dirt().at(cell).wet, 0.75);
    crate::sim::process_pass(&mut state, 0.0);
    assert_eq!(state.dirt().at(cell).wet, 0.75);
    crate::sim::process_pass(&mut state, 2.0 * HOUR / TIME_SCALE);
    assert_eq!(state.dirt().at(cell).wet, 0.25);
    crate::sim::process_pass(&mut state, HOUR / TIME_SCALE);
    assert_eq!(state.dirt().at(cell), Dirt::CLEAN);
}

#[test]
fn a_critical_bladder_empties_once_and_adds_wet_without_erasing_other_stains() {
    let cell = Point::new(0, 0);
    let mut world = World::new(Map::new(Size::new(1, 1), FLOOR));
    let mut human = needy_human(cell, 0.0, 0.0, CRITICAL);
    let stain = Dirt {
        dirt: 0.4,
        blood: 0.3,
        burns: 0.2,
        ..Dirt::CLEAN
    };
    world.dirt.set(cell, stain);
    human.body_mut().set_dirt(stain);
    world.step(&mut human);
    assert_eq!(human.stats().bladder(), 0.0);
    assert_eq!(world.dirt.at(cell), Dirt { wet: 1.0, ..stain });
    assert_eq!(human.body().dirt(), Dirt { wet: 1.0, ..stain });
    for _ in 0..30 {
        world.step(&mut human);
    }
    assert_eq!(
        world
            .lines()
            .iter()
            .filter(|line| line.contains("left a wet puddle"))
            .count(),
        1
    );
    assert!(
        human.stats().bladder() > 0.0,
        "the bladder starts filling again"
    );
    let restored = crate::sim::dirt::DirtLayer::restore(&world.map, &world.dirt.save()).unwrap();
    assert_eq!(restored.at(cell), world.dirt.at(cell));
}

#[test]
fn urgency_alone_a_stopped_bladder_frozen_and_dead_bodies_do_not_make_puddles() {
    let cell = Point::new(0, 0);
    let mut world = World::new(Map::new(Size::new(1, 1), FLOOR));
    let mut human = needy_human(cell, 0.0, 0.0, 99.0);
    world.step(&mut human);
    assert_eq!(world.dirt.at(cell), Dirt::CLEAN);
    human.set_bladder(CRITICAL);
    human
        .biology_mut()
        .unwrap()
        .set_running(ProcessId::Bladder, false);
    world.step(&mut human);
    assert_eq!(human.stats().bladder(), CRITICAL);
    assert_eq!(world.dirt.at(cell), Dirt::CLEAN);
    human
        .biology_mut()
        .unwrap()
        .set_running(ProcessId::Bladder, true);
    human.body_mut().set_frozen(true);
    settle(&mut human, &mut world.facilities, &world.log);
    assert_eq!(world.dirt.at(cell), Dirt::CLEAN);
    human.body_mut().set_frozen(false);
    human.body_mut().set_dead();
    settle(&mut human, &mut world.facilities, &world.log);
    assert_eq!(world.dirt.at(cell), Dirt::CLEAN);
}

#[test]
fn somebody_already_on_a_working_toilet_finishes_without_a_puddle() {
    let cell = Point::new(1, 1);
    let mut map = Map::new(Size::new(3, 3), FLOOR);
    prop_at(&mut map, "toilet", cell);
    let mut world = World::new(map);
    let mut human = needy_human(cell, 0.0, 0.0, CRITICAL);
    for _ in 0..250 {
        world.step(&mut human);
        if world.reliefs() > 0 {
            break;
        }
    }
    assert_eq!(world.reliefs(), 1);
    assert!(human.stats().bladder() < 1.0);
    assert_eq!(world.dirt.at(cell), Dirt::CLEAN);
    assert_eq!(human.body().dirt(), Dirt::CLEAN);
}

#[test]
fn an_unreachable_toilet_does_not_prevent_an_accident() {
    let cell = Point::new(1, 1);
    let mut map = Map::new(Size::new(7, 3), FLOOR);
    for y in 0..3 {
        map.set_terrain(Point::new(3, y), WALL);
    }
    prop_at(&mut map, "toilet", Point::new(5, 1));
    let mut world = World::new(map);
    let mut human = needy_human(cell, 0.0, 0.0, CRITICAL);
    world.step(&mut human);
    assert_eq!(human.stats().bladder(), 0.0);
    assert_eq!(world.dirt.at(cell), PEE);
    assert_eq!(world.reliefs(), 0);
}

#[test]
fn bites_stain_the_victim_and_ground_once_including_a_fatal_bite() {
    let cell = Point::new(1, 1);
    let mut world = World::new(Map::new(Size::new(3, 3), FLOOR));
    let mut human = needy_human(cell, 0.0, 0.0, 0.0);
    let wound = |damage| Wound {
        target: human.uid(),
        by: Uid::new(EntityType::Deathdog, 9),
        from: Point::new(0, 1),
        damage,
    };
    let (first, fatal) = (wound(30), wound(60));
    world.wounds.inflict(first);
    world.step(&mut human);
    assert_eq!(human.body().dirt().blood, 0.3);
    assert_eq!(world.dirt.at(cell).blood, 0.3);
    // No second stain from the wound on the following tick.
    world.dt = 0.0;
    world.step(&mut human);
    assert_eq!(world.dirt.at(cell).blood, 0.3);
    assert_eq!(human.body().dirt().blood, 0.3);
    human
        .biology_mut()
        .unwrap()
        .set_running(ProcessId::Health, false);
    world.wounds.inflict(fatal);
    world.step(&mut human);
    assert_eq!(
        human.body().dirt().blood,
        0.3,
        "disabled health does not bleed"
    );
    human
        .biology_mut()
        .unwrap()
        .set_running(ProcessId::Health, true);
    human
        .biology_mut()
        .unwrap()
        .edit(|stats| stats.with_health(10.0));
    world.wounds.inflict(fatal);
    world.step(&mut human);
    assert!(human.is_dead());
    assert!((human.body().dirt().blood - 0.72).abs() < 0.00001);
    assert_eq!(
        world.corpses.at(cell),
        Some(human.uid()),
        "death's effect was also settled"
    );
    assert!((world.dirt.at(cell).blood - 0.72).abs() < 0.00001);
}

#[test]
fn a_granted_deathdog_bite_leaves_blood_when_the_victim_feels_it() {
    use crate::sim::Effect;
    use crate::sim::kinds::{Deathdog, Facing};
    use rand::{SeedableRng, rngs::SmallRng};

    let map = Map::new(Size::new(10, 10), FLOOR).with_wasteland(5);
    let mut world = World::new(map);
    let cell = (-30..-2)
        .flat_map(|x| (0..10).map(move |y| Point::new(x, y)))
        .find(|&p| {
            world.map.is_passable(p)
                && world.map.is_passable(p.offset(-1, 0))
                && world.dirt.puddle(p).is_none()
        })
        .unwrap();
    let mut human = needy_human(cell, 0.0, 0.0, 0.0);
    world.occupancy.claim(cell, human.uid()).unwrap();
    let mut dog = Deathdog::new(
        Uid::new(EntityType::Deathdog, 9),
        cell.offset(-1, 0),
        Facing::Right,
        &mut SmallRng::seed_from_u64(1),
    );
    crate::sim::effect::apply(
        Effect::Bite {
            at: cell,
            damage: 30,
        },
        &mut dog,
        &mut world.map,
        &mut world.facilities,
        &mut world.occupancy,
        &world.log,
    );
    assert_eq!(world.dirt.at(cell).blood, 0.0, "the bite is still queued");
    world.step(&mut human);
    assert_eq!(world.dirt.at(cell).blood, 0.3);
    assert_eq!(human.body().dirt().blood, 0.3);
}

#[test]
fn accidents_and_bites_are_deterministic_across_parallel_reactions() {
    let build = || {
        let mut state = GameState::new(Map::new(Size::new(32, 32), FLOOR), 17);
        for index in 0..super::super::PARALLEL_AT + 8 {
            let cell = Point::new((index % 30) as i32 + 1, (index / 30) as i32 + 1);
            let uid = state.spawn(EntityType::Human, cell);
            let entity = state.entities.get_mut(uid).unwrap();
            entity
                .biology_mut()
                .unwrap()
                .edit(|stats| stats.with_bladder(CRITICAL));
            state.facilities.wounds.inflict(Wound {
                target: uid,
                by: Uid::new(EntityType::Deathdog, 9),
                from: cell.offset(-1, 0),
                damage: 20,
            });
        }
        state
    };
    let (mut a, mut b) = (build(), build());
    for _ in 0..5 {
        process_game_state(&mut a, 1.0 / 60.0, &Input::new());
        process_game_state(&mut b, 1.0 / 60.0, &Input::new());
        assert_eq!(a.dirt().save(), b.dirt().save());
        let bodies = |state: &GameState| {
            state
                .entities()
                .iter()
                .map(|e| {
                    (
                        e.uid(),
                        e.body().dirt(),
                        e.biology().unwrap().stats().bladder(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(bodies(&a), bodies(&b));
    }
    assert!(a.dirt().save().iter().any(|cell| {
        cell.value
            .as_ref()
            .is_some_and(|v| v.wet > 0.0 && v.blood > 0.0)
    }));
}

#[test]
fn outdoor_spots_and_puddles_are_deterministic_across_parallel_reactions() {
    let build = || {
        let map = Map::new(Size::new(10, 10), FLOOR).with_wasteland(5);
        let mut state = GameState::new(map, 19);
        let mut count = 0;
        for x in -25..-5 {
            for y in 0..20 {
                let cell = Point::new(x, y);
                if !state.map.is_passable(cell) {
                    continue;
                }
                let uid = state.spawn(EntityType::Human, cell);
                state
                    .entity_mut(uid)
                    .unwrap()
                    .biology_mut()
                    .unwrap()
                    .edit(|stats| {
                        stats
                            .with_hunger(0.0)
                            .with_thirst(0.0)
                            .with_bladder(80.0)
                            .with_fun(100.0)
                            .with_stamina(100.0)
                    });
                count += 1;
                if count == super::super::PARALLEL_AT + 8 {
                    return state;
                }
            }
        }
        panic!("not enough outdoor ground");
    };
    let (mut a, mut b) = (build(), build());
    let mut peed = 0;
    for _ in 0..600 {
        process_game_state(&mut a, 1.0 / 60.0, &Input::new());
        process_game_state(&mut b, 1.0 / 60.0, &Input::new());
        assert_eq!(a.dirt().save(), b.dirt().save());
        let units = |state: &GameState| {
            state
                .entities()
                .iter()
                .map(|e| {
                    (
                        e.uid(),
                        e.position(),
                        e.biology().unwrap().stats().bladder(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(units(&a), units(&b));
        peed += a
            .log
            .drain()
            .iter()
            .filter(|line| line.contains("peed outside"))
            .count();
        b.log.drain();
    }
    assert!(peed > 0, "the new task must actually run");
}
