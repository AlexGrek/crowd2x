//! The tests of saving and loading. The one that matters most is the first:
//! a world saved and loaded is the *same* world, tick after tick.

use std::path::PathBuf;

use prost::Message;

use super::*;
use crate::map::grid::power;
use crate::map::{Bounds, GridLayer, Map, Point, Size, FLOOR, WALL};
use crate::sim::biology::Stats;
use crate::sim::testing::prop_at;
use crate::sim::{process_game_state, EntityType, GameState, Input, ItemKind, ProcessId, Uid};

/// The fixed step the game ticks at.
const DT: f32 = 1.0 / 60.0;

fn meta() -> SaveMeta {
    SaveMeta { name: "test".to_string(), map_title: "test map".to_string(), ..SaveMeta::default() }
}

/// Through the whole file and back: encoded, zipped, unzipped, decoded.
fn round_trip(state: &GameState) -> GameState {
    let zip = snapshot(state, &meta(), None).to_zip(None).expect("a save zips");
    restore(&SaveFiles::from_zip(&zip).expect("a save unzips")).expect("a save loads").state
}

#[test]
fn hospital_reservations_rest_nurse_care_and_sick_leave_survive_a_save_exactly() {
    let bed = Point::new(5, 4);
    let mut map = Map::new(Size::new(12, 9), FLOOR);
    prop_at(&mut map, "hospital bed", bed);
    prop_at(&mut map, "nurse station", Point::new(1, 1));
    let mut state = GameState::new(map, 9);
    let nurse = state.spawn(EntityType::Human, Point::new(1, 2));
    let patient = state.spawn(EntityType::Human, Point::new(4, 4));
    set_stats(&mut state, patient, Stats::calm().with_health(30.0).with_hunger(0.0).with_thirst(0.0));
    let mut input = Input::new();
    for uid in [patient, nurse] {
        for p in ProcessId::ALL { if p != ProcessId::Health { input.set_process(uid, p, false); } }
    }
    process_game_state(&mut state, DT, &input);
    run(&mut state, 1200);
    assert!(state.save_world().hospital.iter().any(|b| b.patient == patient.raw()));
    assert!(state.entities().get(patient).unwrap().is_on_sick_leave(state.clock()));
    let copy = side_by_side(&mut state, 5000, |_, _| Input::new());
    assert_eq!(copy.save_world().hospital, state.save_world().hospital);
    assert!(copy.save_world().hospital[0].care > 0);
    // A fully recovered patient is discharged but remembers today's leave.
    set_stats(&mut state, patient, Stats::calm().with_health(100.0));
    run(&mut state, 2);
    assert!(state.save_world().hospital.is_empty());
    let copy = side_by_side(&mut state, 100, |_, _| Input::new());
    assert!(copy.entities().get(patient).unwrap().is_on_sick_leave(copy.clock()));
}

#[test]
fn an_economy_saved_mid_payroll_carries_on_exactly() {
    use crate::sim::economy::{Money, Tariff};
    let mut map = Map::new(Size::new(12, 9), FLOOR);
    prop_at(&mut map, "computer", Point::new(4, 4));
    crate::map::utilities::serve_everything(&mut map, Point::new(0, 8), Point::new(11, 8));
    let mut state = GameState::new(map, 13);
    let clerk = state.spawn(EntityType::Human, Point::new(3, 4));
    let payee = state.spawn(EntityType::Human, Point::new(8, 4));
    let mut input = Input::new();
    for uid in [clerk, payee] {
        for process in ProcessId::ALL { input.set_process(uid, process, false); }
    }
    process_game_state(&mut state, DT, &input);
    // The first unpaid care day reaches the clerk on Tuesday morning.
    for _ in 0..46_000 {
        process_game_state(&mut state, DT, &Input::new());
        if state.entities().get(clerk).unwrap().brain_fields().iter().any(|(key, value)| *key == "task" && value.starts_with("do the payroll")) {
            break;
        }
    }
    assert!(state.economy().owing() > 0);
    assert!(state.entities().get(clerk).unwrap().brain_fields().iter().any(|(key, value)| *key == "task" && value.starts_with("do the payroll")));
    state.set_tariff(Tariff::Price(ItemKind::Food), 150);
    let before = state.economy().account(payee).unwrap().cash;
    let copy = side_by_side(&mut state, 600, |_, _| Input::new());
    assert_eq!(state.economy(), copy.economy());
    assert_eq!(state.economy().account(payee).unwrap().cash, before + Money::dollars(20));
    assert_eq!(copy.economy().tariffs().price(ItemKind::Food), Money(150));
}

#[test]
fn a_save_without_an_economy_opens_accounts_and_refuses_accounts_for_strangers() {
    let mut state = GameState::new(Map::new(Size::new(8, 8), FLOOR), 19);
    let human = state.spawn(EntityType::Human, Point::new(3, 3));
    state.spawn(EntityType::Dog, Point::new(5, 3));
    let mut files = snapshot(&state, &meta(), None);
    files.files.retain(|(name, _)| name != ECONOMY);
    let old = restore(&files).unwrap().state;
    assert_eq!(old.economy().accounts().len(), 1);
    assert_eq!(old.economy().account(human).unwrap().cash.as_cents(), (10 + (human.raw() % 81) as i64) * 100);
    let mut economy = state.economy().save();
    economy.accounts.clear();
    files.files.push((ECONOMY.to_string(), economy.encode_to_vec()));
    assert!(restore(&files).is_err(), "missing account for a living human");
}

#[test]
fn a_damaged_box_and_an_electrician_mid_repair_resume_identically_after_loading() {
    let at = Point::new(3, 3);
    let mut map = Map::new(Size::new(8, 7), FLOOR);
    map.set_grid(GridLayer::Power, at, power::BOX);
    let mut state = GameState::new(map, 42);
    for _ in 0..200 {
        state.switch_box(at, !state.box_is_on(at));
        if state.box_is_damaged(at) { break; }
    }
    assert!(state.box_is_damaged(at));
    let uid = state.spawn(EntityType::Human, Point::new(3, 2));
    let mut input = Input::new();
    for process in ProcessId::ALL { input.set_process(uid, process, false); }
    process_game_state(&mut state, DT, &input);
    for _ in 0..60 { process_game_state(&mut state, DT, &Input::new()); }
    assert!(state.box_is_damaged(at));
    assert_eq!(state.entities().get(uid).unwrap().current_goal(), Some(crate::sim::brain::GoalId::Repair));
    let mut copy = round_trip(&state);
    assert!(copy.box_is_damaged(at));
    for _ in 0..600 {
        process_game_state(&mut state, DT, &Input::new());
        process_game_state(&mut copy, DT, &Input::new());
        assert_eq!(state.save_world(), copy.save_world());
    }
    assert!(!state.box_is_damaged(at));
    assert!(!copy.box_is_damaged(at));
}

#[test]
fn every_worn_outfit_including_none_survives_a_save_and_can_change_after_loading() {
    use crate::sim::clothing::Clothing;
    let mut state = GameState::new(Map::new(Size::new(8, 8), FLOOR), 7);
    let people: Vec<_> = Clothing::ALL.into_iter().map(|outfit| {
        let uid = state.spawn(EntityType::Human, Point::new(3, 3));
        assert!(state.set_clothing(uid, outfit));
        (uid, outfit)
    }).collect();
    let mut copy = round_trip(&state);
    for (uid, outfit) in people {
        assert_eq!(copy.entities().get(uid).unwrap().clothing(), Some(outfit));
        assert!(copy.set_clothing(uid, Clothing::SwimsuitPink));
        assert_eq!(copy.entities().get(uid).unwrap().clothing(), Some(Clothing::SwimsuitPink));
    }
}

#[test]
fn old_saves_recover_the_original_seeded_clothing() {
    use crate::sim::clothing::Clothing;
    let loaded = restore(&SaveFiles::from_zip(FORMAT_1).unwrap()).unwrap().state;
    for person in loaded.entities().iter().filter(|entity| entity.kind() == Some(EntityType::Human)) {
        assert_eq!(person.clothing(), Some(Clothing::initial(person.appearance_seed())));
    }
}

/// Everything a save keeps of a world, as bytes — but the ground loaded
/// round it, which says only what is in memory, and which each world's
/// reclaiming sweeps let go of on their own schedule.
fn fingerprint(state: &GameState) -> Vec<Vec<u8>> {
    let mut parts = vec![state.save_world().encode_to_vec(), state.economy().save().encode_to_vec()];
    parts.extend(state.save_entities(SLOTS_PER_FILE, |shard| shard.encode_to_vec()));
    let salvaged: Vec<_> = state
        .map
        .save_chunks(usize::MAX)
        .into_iter()
        .flat_map(|file| file.chunks)
        .filter(|chunk| !chunk.salvaged.is_empty())
        .map(|chunk| (chunk.x, chunk.y, chunk.salvaged))
        .collect();
    parts.push(format!("{salvaged:?}").into_bytes());
    parts.push(format!("{:?}", state.map.planted().collect::<Vec<_>>()).into_bytes());
    parts
}

/// The two are the same world, or the first entity they disagree about is
/// named.
fn assert_same(a: &GameState, b: &GameState, when: &str) {
    if fingerprint(a) == fingerprint(b) {
        return;
    }
    assert_eq!(a.tick(), b.tick(), "{when}: the clocks");
    for (x, y) in a.entities().iter_slots().zip(b.entities().iter_slots()) {
        assert_eq!(x.0, y.0, "{when}: the arenas");
        let (x, y) = (x.1, y.1);
        assert_eq!(x.uid(), y.uid(), "{when}");
        let bits = |entity: &dyn crate::sim::GameEntity| (entity.position().0.to_bits(), entity.position().1.to_bits());
        assert_eq!(bits(x), bits(y), "{when}: {} is at {:?} and {:?}", x.uid(), x.position(), y.position());
        assert_eq!(x.debug_fields(), y.debug_fields(), "{when}: {}", x.uid());
        assert_eq!(x.brain_fields(), y.brain_fields(), "{when}: {}", x.uid());
        assert_eq!(x.save(), y.save(), "{when}: {}", x.uid());
    }
    assert_eq!(a.save_world(), b.save_world(), "{when}: the world");
    assert_eq!(a.economy(), b.economy(), "{when}: the economy");
    panic!("{when}: the worlds differ in something a save keeps");
}

/// Run `original` and a copy of it saved and loaded side by side for
/// `ticks`, the same commands into both, and check they stay one world.
/// Hands the copy back, to be saved again.
fn side_by_side(original: &mut GameState, ticks: u32, commands: impl Fn(u32, &GameState) -> Input) -> GameState {
    let mut copy = round_trip(original);
    assert_same(original, &copy, "just loaded");
    for tick in 0..ticks {
        let input = commands(tick, original);
        process_game_state(original, DT, &input);
        process_game_state(&mut copy, DT, &input);
        if tick % 20 == 19 {
            assert_same(original, &copy, &format!("{} ticks after loading", tick + 1));
        }
    }
    assert_same(original, &copy, "at the end");
    copy
}

fn run(state: &mut GameState, ticks: u32) {
    let input = Input::new();
    for _ in 0..ticks {
        process_game_state(state, DT, &input);
    }
}

/// Passable ground out in the wasteland that is not walled in by water.
fn dry_cells(map: &Map, near: Point, count: usize) -> Vec<Point> {
    let mut cells: Vec<Point> = Bounds::from_corners(near.offset(-20, -20), near.offset(20, 20))
        .points()
        .filter(|&cell| !map.contains(cell) && map.is_passable(cell) && map.navigation().is_outside(cell))
        .collect();
    cells.sort_by_key(|cell| (cell.manhattan_distance(near), *cell));
    cells.truncate(count);
    assert_eq!(cells.len(), count, "dry ground near {near:?}");
    cells
}

fn set_stats(state: &mut GameState, uid: Uid, stats: Stats) {
    state.entity_mut(uid).unwrap().biology_mut().unwrap().edit(|_| stats);
}

/// A hundred and twenty-eight hungry people on one side of a wall and the
/// fridge on the other, and a few dogs: more far searching at once than the
/// route budget allows, so searches are left half done between ticks. A map
/// with nothing round it, or the crowd would walk round the wall through the
/// wasteland and every search would be cheap; [`busy_district`] is the
/// endless one.
fn crowd_behind_a_wall() -> GameState {
    let mut map = Map::new(Size::new(96, 96), FLOOR);
    for y in 0..95 {
        map.set_terrain(Point::new(48, y), WALL);
    }
    prop_at(&mut map, "fridge", Point::new(90, 2));
    crate::map::utilities::serve_everything(&mut map, Point::new(95, 95), Point::new(94, 95));
    let mut state = GameState::new(map, 11);
    for y in 2..10 {
        for x in 2..18 {
            let uid = state.spawn(EntityType::Human, Point::new(x, y));
            set_stats(
                &mut state,
                uid,
                Stats::calm().with_hunger(95.0).with_thirst(0.0).with_fun(100.0).with_stamina(100.0).with_bladder(0.0),
            );
        }
    }
    for x in 60..66 {
        state.spawn(EntityType::Dog, Point::new(x, 60));
    }
    state
}

#[test]
fn a_saved_world_carries_on_exactly_as_if_it_had_never_been_saved() {
    let mut state = crowd_behind_a_wall();
    // Until somebody is partway through a search the budget cut short: the
    // hardest thing to carry over.
    let mut searching = false;
    for _ in 0..60 {
        run(&mut state, 1);
        searching = state.entities().iter().any(|entity| entity.walker().is_some_and(|walker| walker.is_searching()));
        if searching {
            break;
        }
    }
    assert!(searching, "nobody was left halfway through a search");

    let gone = state.entities().uids().nth(5).unwrap();
    let commands = move |tick: u32, _: &GameState| {
        let mut input = Input::new();
        match tick {
            // Who arrives, and into which hole, is the id allocator's and
            // the free list's to say.
            40 => {
                input.despawn(gone);
            }
            41 => {
                input.spawn(EntityType::Human, Point::new(30, 30));
                input.spawn(EntityType::Dog, Point::new(31, 30));
            }
            _ => {}
        }
        input
    };
    let mut copy = side_by_side(&mut state, 300, commands);
    // A world loaded from a save saves and loads again just as well.
    side_by_side(&mut copy, 300, |_, _| Input::new());
}

/// People starving at one end of a hall and the fridge at the other, each at
/// a pace of its own, saved as they break into a run: how much of a hurry
/// each is in, and how urgent its meal is, come back as they were.
#[test]
fn a_crowd_saved_as_it_breaks_into_a_run_carries_on_exactly() {
    let mut map = Map::new(Size::new(40, 6), FLOOR);
    prop_at(&mut map, "fridge", Point::new(38, 2));
    crate::map::utilities::serve_everything(&mut map, Point::new(0, 5), Point::new(39, 5));
    let mut state = GameState::new(map, 4);
    for y in 1..5 {
        let uid = state.spawn(EntityType::Human, Point::new(1, y));
        set_stats(&mut state, uid, Stats::calm().with_hunger(100.0).with_fun(100.0).with_stamina(100.0));
    }
    let breaking_into_a_run = |state: &GameState| {
        state.entities().iter().any(|entity| entity.walker().is_some_and(|walker| walker.hurrying() > 0.2 && walker.hurrying() < 0.8))
    };
    let mut ticks = 0;
    while !breaking_into_a_run(&state) {
        run(&mut state, 1);
        ticks += 1;
        assert!(ticks < 120, "nobody hurried");
    }
    let paces: Vec<u32> = state.entities().iter().map(|entity| entity.walker().unwrap().speed().to_bits()).collect();
    assert!(paces.iter().any(|&pace| pace != paces[0]), "everybody at one pace");
    side_by_side(&mut state, 300, |_, _| Input::new());
}

/// The king on its way to court, saved and loaded: its stately gait is not
/// saved but comes back from the job, so it walks on exactly as it would have.
#[test]
fn a_king_saved_on_the_way_to_court_walks_on_at_its_stately_pace() {
    let mut map = Map::new(Size::new(40, 6), FLOOR);
    prop_at(&mut map, "throne", Point::new(38, 2));
    let mut state = GameState::new(map, 5);
    let king = state.spawn(EntityType::Human, Point::new(1, 2));
    assert_eq!(state.entities().get(king).unwrap().job().map(|job| job.kind), Some(crate::sim::JobKind::King));
    let mut input = Input::new();
    for process in ProcessId::ALL {
        input.set_process(king, process, false);
    }
    process_game_state(&mut state, DT, &input);
    crate::sim::testing::skip_to(&mut state, 1, 8.6);
    let mut ticks = 0;
    while state.entities().get(king).unwrap().position().0 < 5.0 {
        run(&mut state, 1);
        ticks += 1;
        assert!(ticks < 600, "the king never set off");
    }
    let walker = |state: &GameState| state.entities().get(king).unwrap().walker().unwrap().current_speed();
    assert_eq!(walker(&state), state.entities().get(king).unwrap().walker().unwrap().speed() * crate::sim::job::KING_GAIT);
    let copy = side_by_side(&mut state, 300, |_, _| Input::new());
    assert_eq!(walker(&copy), walker(&state));
}

/// A district — houses, beds and keys, fridges, toilets, doors, street
/// lights — with its residents, and things done to it: somebody frozen, a
/// process switched off, a box and a light switch off, things given and
/// held, two people spawned into one cell, the tired going to bed, and
/// somebody salvaging a heap out in the wasteland.
fn busy_district() -> GameState {
    // The wasteland of seed 3, which has a scrapyard within reach of the
    // site, round district 7.
    let district = crate::map::district::generate(7);
    let mut state = GameState::new(district.map.with_wasteland(3), 7);
    state.spawn_from_spawners();
    let residents: Vec<Uid> = state.entities().uids().collect();
    for &uid in residents.iter().take(4) {
        set_stats(&mut state, uid, Stats::calm().with_stamina(5.0).with_hunger(0.0).with_thirst(0.0).with_bladder(0.0));
    }

    let mut heaps = Vec::new();
    let site = state.map.site();
    state.map.heaps(site.grown(80), |cell, _| heaps.push(cell));
    let map = &state.map;
    heaps.retain(|heap| heap.cardinal_neighbours().iter().any(|&side| map.is_passable(side) && map.navigation().is_outside(side)));
    let heap = *heaps.iter().min_by_key(|heap| (heap.manhattan_distance(site.origin), **heap)).expect("a heap of scrap near the district");
    let beside = *heap
        .cardinal_neighbours()
        .iter()
        .find(|&&side| map.is_passable(side) && map.navigation().is_outside(side))
        .unwrap();
    let salvager = state.spawn(EntityType::Human, beside);
    let wanderers = dry_cells(&state.map, Point::new(-25, 20), 5);
    for &cell in &wanderers[..3] {
        state.spawn(EntityType::Dog, cell);
    }
    let stacked = wanderers[3];
    state.spawn(EntityType::Human, stacked);
    state.spawn(EntityType::Human, stacked);

    let a_box = (0..state.map.size().height)
        .flat_map(|y| (0..state.map.size().width).map(move |x| Point::new(x, y)))
        .find(|&cell| state.map.grid(GridLayer::Power, cell) == power::BOX)
        .expect("a district has distribution boxes");
    let a_switch = state.light_switches().cells().next();

    let mut input = Input::new();
    input
        .salvage(salvager, heap)
        .freeze(residents[5], true)
        .set_process(residents[6], ProcessId::Hunger, false)
        .give_item(residents[7], ItemKind::Water)
        .hold(residents[8], Some(ItemKind::Food))
        .switch_box(a_box, false);
    if let Some(at) = a_switch {
        input.switch_light(at, false);
    }
    process_game_state(&mut state, DT, &input);
    state
}

#[test]
fn a_district_saved_in_the_middle_of_everything_carries_on_exactly() {
    let mut state = busy_district();
    run(&mut state, 600);
    let salvaged = state.map.save_chunks(usize::MAX).iter().flat_map(|file| &file.chunks).any(|chunk| !chunk.salvaged.is_empty());
    assert!(salvaged, "nobody has salvaged anything yet");
    assert!(
        state.entities().iter().any(|entity| matches!(entity.brain_fields().first(), Some((_, goal)) if goal == "sleep")),
        "nobody is going to bed"
    );
    side_by_side(&mut state, 600, |_, _| Input::new());
}

/// The same district on a Monday morning, saved with the bank's desks
/// taken and the gates' day watch at their posts: workers on their way in
/// and at their desks, hours half worked, one let go and one hired while
/// the two worlds run.
#[test]
fn a_district_saved_in_the_middle_of_a_working_day_carries_on_exactly() {
    let mut state = busy_district();
    let doing = |state: &GameState, task: &str| {
        state.entities().iter().any(|entity| {
            entity.brain_fields().iter().any(|(name, value)| *name == "task" && value.starts_with(task))
        })
    };
    let at_work = |state: &GameState| doing(state, "work at the computer") && doing(state, "keep watch at the guard post");
    let mut ticks = 0;
    while !at_work(&state) {
        run(&mut state, 60);
        ticks += 60;
        assert!(ticks < 4000, "nobody sat down at a desk or stood at a post by {}", state.clock());
    }
    // Into the hour, so a stretch is part done when it is saved.
    run(&mut state, 300);
    assert!(state.workplaces().len() >= 12, "{} desks given out", state.workplaces().len());
    let workers: Vec<Uid> = state.entities().iter().filter(|entity| entity.job().is_some()).map(|entity| entity.uid()).collect();
    let idle = state.entities().iter().find(|entity| entity.job().is_none() && entity.kind() == Some(EntityType::Human)).map(|entity| entity.uid());
    side_by_side(&mut state, 600, |tick, _| {
        let mut input = Input::new();
        if tick == 100 {
            input.dismiss(workers[0]);
        }
        if tick == 200
            && let Some(idle) = idle
        {
            input.hire(idle);
        }
        input
    });
}


/// A district's playzone in the middle of Monday: its administrators at the
/// computer and some bored residents let in to play — saved while the
/// cabinets work, a game half played and the shift part done — carries on
/// exactly, the administrators' word that the playzone is run (the world's
/// `playzones_run`) included. The administrators are let go in the middle of
/// it, which turns every cabinet dark at once in both worlds.
#[test]
fn a_playzone_saved_with_games_being_played_carries_on_exactly() {
    use crate::map::district::playzone::{CABINET, NAME};
    use crate::map::ObjectLayer;
    use crate::sim::JobKind;

    let district = crate::map::district::generate(7);
    let mut state = GameState::new(district.map.clone(), 7);
    state.spawn_from_spawners();
    let administrators: Vec<Uid> = state
        .entities()
        .iter()
        .filter(|entity| entity.job().is_some_and(|job| job.kind == JobKind::PlayzoneAdministrator))
        .map(|entity| entity.uid())
        .collect();
    assert_eq!(administrators.len(), 4);
    let mut input = Input::new();
    for &uid in &administrators {
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
            input.set_process(uid, process, false);
        }
    }
    process_game_state(&mut state, DT, &input);

    // Three more, bored and nothing else, inside the playzone itself.
    let walls = district.the(NAME);
    let props = state.map.objects(ObjectLayer::Props).to_vec();
    let free: Vec<Point> = walls
        .interior()
        .cells()
        .filter(|&cell| state.map.is_passable(cell) && !props.iter().any(|prop| prop.cell() == cell))
        .collect();
    let players: Vec<Uid> = free.iter().take(3).map(|&cell| state.spawn(EntityType::Human, cell)).collect();
    for &uid in &players {
        set_stats(&mut state, uid, Stats::calm().with_fun(5.0));
    }
    assert_eq!(props.iter().filter(|prop| prop.kind.as_str() == CABINET && walls.interior().contains(prop.cell())).count(), 6);

    // To the middle of the day, and on until somebody is playing a cabinet.
    run(&mut state, 3 * 1800 + 300);
    let playing = |state: &GameState| {
        state.entities().iter().any(|entity| {
            entity.interacting_with().is_some_and(|cell| props.iter().any(|prop| prop.cell() == cell && prop.kind.as_str() == CABINET))
        })
    };
    let mut ticks = 0;
    while !(playing(&state) && state.playzones().working() == 6) {
        run(&mut state, 30);
        ticks += 30;
        assert!(ticks < 6000, "nobody played by {}: {} cabinets working", state.clock(), state.playzones().working());
    }
    assert!(!state.playzones().running().is_empty());
    let saved = state.save_world();
    assert_eq!(saved.playzones_run.len(), 1, "the one computer, run");

    side_by_side(&mut state, 600, |tick, _| {
        let mut input = Input::new();
        if tick == 300 {
            input.dismiss(administrators[0]);
            input.dismiss(administrators[2]);
        }
        input
    });
}

/// The same district once the scrap collectors have stored a packful:
/// saved with scrap in the warehouse's bins and collectors out at the heaps
/// with some in their packs, mid-trip — a piece half worked loose, a walk
/// half walked — and one let go while the two worlds run.
#[test]
fn a_district_saved_with_scrap_in_its_bins_and_its_collectors_out_carries_on_exactly() {
    use crate::sim::JobKind;
    let mut state = busy_district();
    let collectors: Vec<Uid> = state
        .entities()
        .iter()
        .filter(|entity| entity.job().is_some_and(|job| job.kind == JobKind::ScrapCollector))
        .map(|entity| entity.uid())
        .collect();
    assert_eq!(collectors.len(), crate::map::district::warehouse::WAREHOUSE_BINS);
    // Only the job wanted, so a morning is enough for a trip.
    let mut input = Input::new();
    for &uid in &collectors {
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
            input.set_process(uid, process, false);
        }
    }
    process_game_state(&mut state, DT, &input);
    let carrying = |state: &GameState| {
        collectors
            .iter()
            .filter(|&&uid| state.entities().get(uid).unwrap().inventory().is_some_and(|i| i.count(ItemKind::Scrap) > 0))
            .count()
    };
    let mut ticks = 0;
    while state.bins().total() == 0 || carrying(&state) == 0 {
        run(&mut state, 60);
        ticks += 60;
        assert!(ticks < 9 * 3600, "no scrap stored and more carried by {}", state.clock());
    }
    let stored = state.bins().total();
    let copy = side_by_side(&mut state, 900, |tick, _| {
        let mut input = Input::new();
        if tick == 300 {
            input.dismiss(collectors[0]);
        }
        input
    });
    assert!(copy.bins().total() >= stored);
    assert_eq!(copy.bins(), state.bins());
}

/// Two dead, one of them already in an undertaker's hands and the other
/// still lying where it fell: the dead stay dead, the one carried is still
/// carried and the one lying is still there to be found, and the world goes
/// on as it would have.
#[test]
fn a_world_saved_with_the_dead_lying_and_carried_carries_on_exactly() {
    let mut map = Map::new(Size::new(20, 12), FLOOR);
    prop_at(&mut map, "crematorium", Point::new(1, 1));
    let mut state = GameState::new(map, 5);
    let undertaker = state.spawn(EntityType::Human, Point::new(2, 1));
    let dead: Vec<Uid> = [Point::new(9, 5), Point::new(18, 10)].into_iter().map(|at| state.spawn(EntityType::Human, at)).collect();
    let mut input = Input::new();
    for &uid in &dead {
        assert!(state.dismiss(uid));
        set_stats(&mut state, uid, Stats::calm().with_health(0.5).with_hunger(100.0).with_thirst(100.0));
    }
    for process in ProcessId::ALL {
        input.set_process(undertaker, process, false);
    }
    process_game_state(&mut state, DT, &input);
    let carrying = |state: &GameState| {
        state.entities().get(undertaker).unwrap().inventory().unwrap().hand() == Some(ItemKind::Body)
    };
    let mut ticks = 0;
    while !carrying(&state) {
        run(&mut state, 10);
        ticks += 10;
        assert!(ticks < 3 * 3600, "nobody taken up by {}", state.clock());
    }
    assert_eq!(state.corpses().len(), 1, "one carried, one lying");
    let stats = state.statistics();
    assert_eq!((stats.died, stats.lying, stats.carried), (2, 1, 1));
    let copy = side_by_side(&mut state, 600, |_, _| Input::new());
    assert_eq!(copy.corpses(), state.corpses());
    assert_eq!(copy.tally(), state.tally(), "the dead counted are counted still");
    assert_eq!(copy.statistics(), state.statistics());
}

/// The steel works mid-shift: scrap in the warehouse's bin and the
/// factory's hopper, a steelworker at its furnace or on the way to it, a
/// courier with a load. What every store holds, what every pack holds, and
/// where each is in its work all come back, and the world goes on as it would
/// have.
#[test]
fn the_steel_works_saved_mid_shift_carries_on_exactly() {
    let (mut state, smelter, courier) = crate::sim::testing::steel_works(12, 9);
    let hopper = Point::new(16, 2);
    let mut ticks = 0;
    // Until the courier has filled the hopper once and the steelworker is
    // at the furnace with what it took from it.
    while state.bins().stored(hopper) == 0 || state.entities().get(smelter).unwrap().interacting_with() != Some(Point::new(21, 5)) {
        run(&mut state, 10);
        ticks += 10;
        assert!(ticks < 9 * 1800, "nothing smelted by {}", state.clock());
    }
    let copy = side_by_side(&mut state, 1800, |_, _| Input::new());
    assert_eq!(copy.bins(), state.bins());
    let pack = |state: &GameState, uid: Uid| state.entities().get(uid).unwrap().inventory().unwrap().describe_load();
    assert_eq!(pack(&copy, courier), pack(&state, courier));
    assert!(state.bins().iter().map(|(_, stored)| stored).sum::<u32>() < 12, "some of it carried or smelted");
}

#[test]
fn the_food_factory_saved_while_cooking_carries_on_exactly() {
    let (mut state, worker, _) = crate::sim::testing::food_works(12, 9);
    let mut ticks = 0;
    while state.entities().get(worker).unwrap().interacting_with() != Some(Point::new(21, 5)) {
        run(&mut state, 10);
        ticks += 10;
        assert!(ticks < 6 * 1800, "no cooking by {}", state.clock());
    }
    let copy = side_by_side(&mut state, 5400, |_, _| Input::new());
    assert_eq!(copy.bins(), state.bins());
    assert!(copy.bins().total_in(crate::sim::feature::FeatureKind::MealRack) > 0);
}

#[test]
fn loading_a_district_preserves_current_meals_without_restocking_it() {
    let mut state = GameState::new(crate::map::district::generate(7).map, 7);
    let rack = state.features().cells_of(crate::sim::feature::FeatureKind::MealRack)[0];
    assert_eq!(state.bins().stored(rack), 30);
    let mut saved = state.save_world();
    saved.bins.iter_mut().find(|bin| bin.cell == Some(rack.into())).unwrap().stored = 23;
    state = GameState::restore(state.map.clone(), &saved, Vec::new()).unwrap();
    let copy = side_by_side(&mut state, 1, |_, _| Input::new());
    assert_eq!(copy.bins().stored(rack), 23);
    assert_eq!(copy.map.initial_stock(), state.map.initial_stock());
}

/// A farm in the middle of the week: its field sown on the Monday, the
/// Tuesday's tending under way. Every crop — where, how old, how well kept,
/// when last tended — comes back, planted in the map again, and the world
/// goes on as it would have: the rest of the field tended after the load.
#[test]
fn a_farm_saved_mid_week_carries_on_exactly() {
    let (mut state, farmer) = crate::sim::testing::farm(5);
    let mut ticks = 0;
    while state.fields().len() < crate::sim::fields::PLOTS {
        run(&mut state, 10);
        ticks += 10;
        assert!(ticks < 6 * 1800, "nothing sown by {}", state.clock());
    }
    crate::sim::testing::skip_to(&mut state, 2, 9.0);
    while !state.fields().iter().any(|crop| crop.tended == 2)
        || state.entities().get(farmer).unwrap().interacting_with().is_none()
    {
        run(&mut state, 10);
        ticks += 10;
        assert!(ticks < 12 * 1800, "nothing tended by {}", state.clock());
    }
    let copy = side_by_side(&mut state, 3600, |_, _| Input::new());
    let crops = |state: &GameState| state.fields().iter().copied().collect::<Vec<_>>();
    assert_eq!(crops(&copy), crops(&state));
    assert_eq!(copy.map.planted().collect::<Vec<_>>(), state.map.planted().collect::<Vec<_>>());
    assert!(state.fields().iter().filter(|crop| crop.tended == 2).count() > 1, "more tended after the load");
}

/// A scrap collector saved while it gets ready for a trip out — at the
/// fridge, the TV or the toilet, with needs still on its list — comes back
/// part way through the same list, and goes on to the same trip.
#[test]
fn a_collector_saved_while_it_gets_ready_for_a_trip_carries_on_exactly() {
    let mut map = Map::new(Size::new(9, 7), FLOOR).with_wasteland(0);
    for (name, cell) in [("scrap bin", Point::new(4, 3)), ("fridge", Point::new(7, 1)), ("tv", Point::new(7, 3)), ("toilet", Point::new(7, 5))] {
        prop_at(&mut map, name, cell);
    }
    crate::map::utilities::serve_everything(&mut map, Point::new(0, 6), Point::new(8, 6));
    let mut state = GameState::new(map, 3);
    let collector = state.spawn(EntityType::Human, Point::new(1, 1));
    assert_eq!(state.entities().get(collector).unwrap().job().map(|job| job.kind), Some(crate::sim::JobKind::ScrapCollector));
    let mut ticks = 0;
    while !state.entities().get(collector).unwrap().is_getting_ready() {
        run(&mut state, 10);
        ticks += 10;
        assert!(ticks < 9 * 1800, "never got ready by {}", state.clock());
    }
    run(&mut state, 200);
    assert!(state.entities().get(collector).unwrap().is_getting_ready(), "done in 200 ticks: save it sooner");
    let mut copy = side_by_side(&mut state, 3600, |_, _| Input::new());
    assert!(!copy.entities().get(collector).unwrap().is_getting_ready(), "still getting ready after two hours");
    let ready = |state: &mut GameState| state.log.drain().iter().any(|line| line.contains("is ready to go out"));
    assert!(ready(&mut copy) && ready(&mut state), "both set out");
}

#[test]
fn saving_a_loaded_world_gives_back_the_same_files() {
    let mut state = busy_district();
    run(&mut state, 200);
    let first = snapshot(&state, &meta(), None);
    let second = snapshot(&restore(&first).unwrap().state, &meta(), None);
    let names = |files: &SaveFiles| files.files.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&first), names(&second));
    for ((name, a), (_, b)) in first.files.iter().zip(&second.files) {
        assert!(a == b, "{name} changed between one save and the next");
    }
}

#[test]
fn a_save_holds_the_files_it_says_it_does() {
    let state = fixture_world();
    let view = pb::View { camera_x: 10.0, camera_y: -4.0, zoom: 3, speed: 2.0, paused: true, ..Default::default() };
    let files = snapshot(&state, &meta(), Some(&view));
    let names: Vec<&str> = files.files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(&names[..3], [MANIFEST, MAP, WORLD]);
    assert!(names.contains(&"entities/0000.pb") && names.contains(&"chunks/0000.pb") && names.contains(&VIEW));

    let zip = files.to_zip(None).unwrap();
    let loaded = restore(&SaveFiles::from_zip(&zip).unwrap()).unwrap();
    assert_eq!(loaded.view, Some(view));
    assert_eq!(loaded.manifest.name, "test");
    assert_eq!(loaded.manifest.population, state.len() as u64);
    assert_eq!(archive::read_manifest(std::io::Cursor::new(&zip)).unwrap(), loaded.manifest);
}

/// The files of a small saved world, to break.
fn small_save() -> SaveFiles {
    let mut state = crowd_behind_a_wall();
    run(&mut state, 3);
    snapshot(&state, &meta(), None)
}

fn replace(files: &mut SaveFiles, name: &str, bytes: Vec<u8>) {
    files.files.iter_mut().find(|(file, _)| file == name).expect("the file is there").1 = bytes;
}

fn refused(files: &SaveFiles) -> String {
    match restore(files) {
        Ok(_) => panic!("a broken save loaded"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn what_is_not_a_zip_is_refused() {
    assert!(matches!(SaveFiles::from_zip(b"not a zip at all"), Err(SaveError::Zip(_))));
    let mut zip = small_save().to_zip(None).unwrap();
    zip.truncate(zip.len() / 2);
    assert!(SaveFiles::from_zip(&zip).is_err(), "half a zip");
}

#[test]
fn a_save_missing_its_world_is_refused() {
    let mut files = small_save();
    files.files.retain(|(name, _)| name != WORLD);
    assert!(matches!(restore(&files), Err(SaveError::Missing(file)) if file == WORLD));
}

#[test]
fn a_broken_file_is_refused_by_name() {
    let mut files = small_save();
    replace(&mut files, WORLD, vec![0xff, 0xff, 0xff]);
    assert!(matches!(restore(&files), Err(SaveError::Decode(file, _)) if file == WORLD));
}

#[test]
fn a_save_from_a_newer_format_is_refused() {
    let mut files = small_save();
    let mut manifest = pb::Manifest::decode(files.get(MANIFEST).unwrap()).unwrap();
    manifest.format = FORMAT + 1;
    replace(&mut files, MANIFEST, manifest.encode_to_vec());
    assert!(matches!(restore(&files), Err(SaveError::NewerFormat { found, .. }) if found == FORMAT + 1));
}

#[test]
fn terrain_this_build_does_not_know_is_refused() {
    let mut files = small_save();
    let mut site = pb::SiteMap::decode(files.get(MAP).unwrap()).unwrap();
    site.terrain_names[0] = "lava".to_string();
    replace(&mut files, MAP, site.encode_to_vec());
    let error = refused(&files);
    assert!(error.contains("map.pb") && error.contains("lava"), "{error}");
}

fn first_shard(files: &SaveFiles) -> pb::EntityShard {
    pb::EntityShard::decode(files.get("entities/0000.pb").unwrap()).unwrap()
}

#[test]
fn an_unknown_outfit_in_a_save_is_refused_by_name() {
    let mut files = small_save();
    let mut shard = first_shard(&files);
    let human = shard.slots.iter_mut().find_map(|record| match &mut record.kind {
        Some(pb::Kind::Human(human)) => Some(human),
        _ => None,
    }).unwrap();
    human.clothing = Some("missing outfit".to_string());
    replace(&mut files, "entities/0000.pb", shard.encode_to_vec());
    let error = refused(&files);
    assert!(error.contains("entities/0000.pb") && error.contains("missing outfit"), "{error}");
}

#[test]
fn an_entity_saved_as_a_kind_its_id_disagrees_with_is_refused() {
    let mut files = small_save();
    let mut shard = first_shard(&files);
    let record = shard.slots.iter_mut().find(|record| matches!(record.kind, Some(pb::Kind::Human(_)))).unwrap();
    let Some(pb::Kind::Human(human)) = &mut record.kind else { unreachable!() };
    human.walker.as_mut().unwrap().body.as_mut().unwrap().uid = Uid::new(EntityType::Dog, 99).raw();
    replace(&mut files, "entities/0000.pb", shard.encode_to_vec());
    let error = refused(&files);
    assert!(error.contains("entities/0000.pb") && error.contains("saved as a human"), "{error}");
}

#[test]
fn one_entity_in_two_slots_is_refused() {
    let mut files = small_save();
    let mut shard = first_shard(&files);
    let mut twin = shard.slots[0].clone();
    twin.slot = shard.slots.last().unwrap().slot + 1;
    shard.slots.push(twin);
    replace(&mut files, "entities/0000.pb", shard.encode_to_vec());
    let error = refused(&files);
    assert!(error.contains("two slots") || error.contains("slot"), "{error}");
}

#[test]
fn files_and_fields_from_a_later_build_are_skipped() {
    let files = small_save();
    let mut later = files.clone();
    later.files.push(("weather.pb".to_string(), vec![1, 2, 3]));
    // Field 99, a varint: something a later build keeps in the world.
    let mut world = later.get(WORLD).unwrap().to_vec();
    world.extend([0x98, 0x06, 0x01]);
    replace(&mut later, WORLD, world);
    assert_eq!(fingerprint(&restore(&later).unwrap().state), fingerprint(&restore(&files).unwrap().state));
}

#[test]
fn a_save_without_a_view_loads_looking_nowhere_in_particular() {
    assert_eq!(restore(&small_save()).unwrap().view, None);
}

// --- the store ---

/// A directory that deletes itself.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("crowd2x-saves-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn moment(seconds: i64) -> store::Moment {
    use chrono::TimeZone;
    store::Moment::at(chrono::Utc.timestamp_opt(1_790_000_000 + seconds, 0).unwrap())
}

fn zipped(name: &str, quick: bool, at: &store::Moment) -> Vec<u8> {
    let manifest = pb::Manifest {
        format: FORMAT,
        name: name.to_string(),
        quick,
        created_unix_ms: at.unix_ms,
        created_local: at.local.clone(),
        ..pb::Manifest::default()
    };
    SaveFiles { files: vec![(MANIFEST.to_string(), manifest.encode_to_vec())] }.to_zip(Some(at.stamp)).unwrap()
}

#[test]
fn a_save_is_named_after_when_it_was_made_and_never_overwrites_another() {
    let dir = TempDir::new("names");
    let store = SaveStore::new(&dir.0);
    let at = moment(0);
    let first = store.write(&zipped("one", false, &at), &at).unwrap();
    let second = store.write(&zipped("two", false, &at), &at).unwrap();
    assert_eq!(first.file_name().unwrap().to_str().unwrap(), format!("{}.savegame", at.stem));
    assert_eq!(second.file_name().unwrap().to_str().unwrap(), format!("{}-2.savegame", at.stem));
    assert!(!at.stem.contains(':'), "a file name Windows takes: {}", at.stem);
    let leftovers: Vec<_> = std::fs::read_dir(&dir.0).unwrap().flatten().filter(|entry| entry.path().extension().is_some_and(|ext| ext == "tmp")).collect();
    assert!(leftovers.is_empty(), "a temporary file was left behind");
}

#[test]
fn saves_are_listed_newest_first_and_a_broken_one_is_listed_with_why() {
    let dir = TempDir::new("list");
    let store = SaveStore::new(&dir.0);
    assert!(store.list().unwrap().is_empty(), "no directory is no saves");
    for (seconds, name) in [(10, "middle"), (0, "oldest"), (20, "newest")] {
        let at = moment(seconds);
        store.write(&zipped(name, false, &at), &at).unwrap();
    }
    std::fs::write(dir.0.join("junk.savegame"), b"not a save").unwrap();
    let listed = store.list().unwrap();
    let names: Vec<String> = listed.iter().filter_map(|save| save.manifest.as_ref().ok().map(|m| m.name.clone())).collect();
    assert_eq!(names, ["newest", "middle", "oldest"]);
    assert_eq!(listed.iter().filter(|save| !save.is_loadable()).count(), 1);
    assert_eq!(store.newest().unwrap().unwrap().manifest.unwrap().name, "newest");
}

#[test]
fn only_the_newest_quick_saves_are_kept_and_never_one_the_player_named() {
    let dir = TempDir::new("quick");
    let store = SaveStore::new(&dir.0);
    let at = moment(0);
    store.write(&zipped("quick save", false, &at), &at).unwrap();
    for seconds in 1..=6 {
        let at = moment(seconds);
        store.write(&zipped("quick save", true, &at), &at).unwrap();
        store.prune_quick(QUICK_SAVES_KEPT).unwrap();
    }
    let listed = store.list().unwrap();
    let quick: Vec<i64> = listed.iter().filter(|save| save.is_quick()).map(|save| save.manifest.as_ref().unwrap().created_unix_ms).collect();
    assert_eq!(quick, (3..=6).rev().map(|seconds| moment(seconds).unix_ms).collect::<Vec<_>>());
    assert_eq!(listed.len(), QUICK_SAVES_KEPT + 1, "the save the player named stays");
}

// --- the file format, kept ---

/// A save made by format 1, checked in: it must keep loading, whatever the
/// schema grows. Rewrite it only with a format change, beside a new one.
const FORMAT_1: &[u8] = include_bytes!("testdata/format1.savegame");

/// The world in the fixture: small, on an endless map, with a bit of
/// everything in it.
fn fixture_world() -> GameState {
    let mut map = Map::new(Size::new(24, 16), FLOOR).with_wasteland(3);
    for x in 0..24 {
        map.set_terrain(Point::new(x, 8), WALL);
    }
    map.set_terrain(Point::new(12, 8), FLOOR);
    prop_at(&mut map, "fridge", Point::new(20, 12));
    prop_at(&mut map, "toilet", Point::new(3, 13));
    crate::map::utilities::serve_everything(&mut map, Point::new(23, 15), Point::new(22, 15));
    let mut state = GameState::new(map, 21);
    for x in 2..7 {
        state.spawn(EntityType::Human, Point::new(x, 2));
    }
    for cell in dry_cells(&state.map, Point::new(-10, 5), 2) {
        state.spawn(EntityType::Dog, cell);
    }
    run(&mut state, 300);
    state
}

/// Writes the fixture. `cargo test write_the_format_1_fixture -- --ignored`,
/// once, when a format is new.
#[test]
#[ignore]
fn write_the_format_1_fixture() {
    let zip = snapshot(&fixture_world(), &meta(), None).to_zip(None).unwrap();
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/save/testdata/format1.savegame"), zip).unwrap();
}

#[test]
fn a_format_1_save_still_loads_and_still_runs() {
    let mut loaded = restore(&SaveFiles::from_zip(FORMAT_1).unwrap()).expect("format 1 loads").state;
    assert_eq!(loaded.tick(), 300);
    assert_eq!(loaded.len(), 7);
    run(&mut loaded, 60);
    assert_eq!(loaded.tick(), 360);
}

/// A steel works saved mid-shift by the build before the kinds were
/// catalogued (`sim::catalogue`): jobs and goals written as the old schema's
/// enums, every task under its own tag, a courier and a steelworker in
/// uniform. It must keep loading, and go on exactly as the same world built
/// fresh does.
const STEEL_WORKS: &[u8] = include_bytes!("testdata/steel_works.savegame");

/// The one pace everybody walked at when [`STEEL_WORKS`] was written, before
/// each person had a pace of their own: two cells a second. A number and not
/// `kinds::HUMAN_SPEED`, which has been doubled since — and the save keeps
/// each walker's pace, so the world loads at this one whatever the constant.
const STEEL_WORKS_PACE: f32 = 2.0;

/// The world in that fixture: [`crate::sim::testing::steel_works`] run until
/// the steelworker is at its furnace with scrap the courier brought, at
/// [`STEEL_WORKS_PACE`].
fn steel_works_fixture() -> GameState {
    let (mut state, smelter, _) = crate::sim::testing::steel_works(12, 9);
    for uid in state.entities().uids().collect::<Vec<_>>() {
        state.entity_mut(uid).unwrap().walker_mut().unwrap().set_speed(STEEL_WORKS_PACE);
    }
    while state.bins().stored(Point::new(16, 2)) == 0 || state.entities().get(smelter).unwrap().interacting_with() != Some(Point::new(21, 5)) {
        run(&mut state, 10);
    }
    state
}

/// Writes it. `cargo test write_the_steel_works_fixture -- --ignored`, once.
#[test]
#[ignore]
fn write_the_steel_works_fixture() {
    let zip = snapshot(&steel_works_fixture(), &meta(), None).to_zip(None).unwrap();
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/save/testdata/steel_works.savegame"), zip).unwrap();
}

#[test]
fn a_steel_works_saved_before_the_catalogues_loads_and_goes_on_exactly() {
    let mut loaded = restore(&SaveFiles::from_zip(STEEL_WORKS).unwrap()).expect("it loads").state;
    // The old fixture predates money and timesheets. Put the freshly replayed
    // reference through that same migration, while still comparing every
    // pre-existing brain, entity and world field with the golden archive.
    let reference = steel_works_fixture();
    // This archive predates fully healthy arrivals. Preserve its original
    // birth health in the reference, while checking every other saved field.
    let birth_health: std::collections::BTreeMap<_, _> = loaded.entities().iter()
        .filter_map(|e| e.biology().map(|b| (e.uid().raw(), b.stats().health()))).collect();
    let mut files = snapshot(&reference, &meta(), None);
    files.files.retain(|(name, _)| name != ECONOMY);
    for (name, bytes) in &mut files.files {
        if !name.starts_with(ENTITIES) { continue; }
        let mut shard = pb::EntityShard::decode(bytes.as_slice()).unwrap();
        for slot in &mut shard.slots {
            if let Some(pb::Kind::Human(human)) = &mut slot.kind {
                human.brain.as_mut().unwrap().timesheet = None;
                let uid = human.walker.as_ref().unwrap().body.as_ref().unwrap().uid;
                human.biology.as_mut().unwrap().stats.as_mut().unwrap().health = birth_health[&uid];
            }
        }
        *bytes = shard.encode_to_vec();
    }
    let mut fresh = restore(&files).unwrap().state;
    assert_same(&fresh, &loaded, "just loaded");
    for tick in 0..1800 {
        run(&mut fresh, 1);
        run(&mut loaded, 1);
        if tick % 60 == 59 {
            assert_same(&fresh, &loaded, &format!("{} ticks after loading", tick + 1));
        }
    }
}

// --- how fast ---

/// What a save of a big crowd costs, half by half. Run it optimised:
/// `cargo test --release saving_twenty_thousand -- --ignored --nocapture`.
#[test]
#[ignore]
fn saving_twenty_thousand_humans() {
    let map = Map::new(Size::new(256, 256), FLOOR).with_wasteland(1);
    let mut state = GameState::new(map, 1);
    for index in 0..20_000 {
        state.spawn(EntityType::Human, Point::new(index % 250 + 3, index / 250 + 3));
    }
    run(&mut state, 30);
    let started = std::time::Instant::now();
    let files = snapshot(&state, &meta(), None);
    let encoded = started.elapsed();
    let zip = files.to_zip(None).unwrap();
    let zipped = started.elapsed() - encoded;
    let unzipped = SaveFiles::from_zip(&zip).unwrap();
    let loaded = restore(&unzipped).unwrap();
    let total = started.elapsed();
    println!(
        "20000 humans: {} files, {:.1} MB raw, {:.1} MB zipped; encode {:.1} ms, compress {:.1} ms, unzip and load {:.1} ms",
        files.files.len(),
        files.raw_size() as f64 / 1e6,
        zip.len() as f64 / 1e6,
        encoded.as_secs_f64() * 1e3,
        zipped.as_secs_f64() * 1e3,
        (total - encoded - zipped).as_secs_f64() * 1e3,
    );
    assert_eq!(loaded.state.len(), 20_000);
}
