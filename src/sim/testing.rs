//! A world of one, for tests about what a single unit decides.
//!
//! `GameState` cannot hand a test the `Human` behind a `dyn GameEntity` — the
//! entity table is deliberately not downcastable — so a test that needs to set
//! somebody's hunger and then watch them runs them here instead: the same
//! three steps as `process_pass`, over one entity, with the move step's rules
//! applied by hand.

use rand::rngs::SmallRng;
use rand::SeedableRng;

use crate::map::utilities::Supply;
use crate::map::{Map, Object, ObjectKind, ObjectLayer, Point, PIXELS_PER_CELL};

use super::clock::Clock;
use super::entity::{cell_of, GameEntity, Think};
use super::facilities::Facilities;
use super::kinds::Human;
use super::log::Log;
use super::occupancy::Occupancy;
use super::uid::{EntityType, Uid};
use super::{Effect, Intent, MoveOutcome};

pub struct World {
    pub map: Map,
    pub occupancy: Occupancy,
    pub log: Log,
    /// Everything a [`super::GameState`] keeps beside its map and its crowd,
    /// the same struct — the features, the fridges, the stores, the doors,
    /// the properties, the dead — built from `map` once; [`World::step`]
    /// grants a react'd [`Effect`] to it as [`super::world_step`] does
    /// ([`super::effect::apply`]). A `World` derefs to it, so a test reads
    /// `world.bins` and sets `world.features`.
    pub facilities: Facilities,
    pub tick: u64,
    pub dt: f32,
    /// What time it is. Opening time, until a test sets it: a test about
    /// sleeping at night puts it there.
    pub clock: Clock,
    /// Everything the log has said, kept: the log itself is drained.
    lines: Vec<String>,
}

impl std::ops::Deref for World {
    type Target = Facilities;

    fn deref(&self) -> &Facilities {
        &self.facilities
    }
}

impl std::ops::DerefMut for World {
    fn deref_mut(&mut self) -> &mut Facilities {
        &mut self.facilities
    }
}

impl World {
    /// A world where everything on `map` works, plugged in or not: the
    /// tests run here are about what a unit decides, and a fridge in one of
    /// them is a fridge. [`World::as_built`] is the one that asks the wiring.
    pub fn new(map: Map) -> World {
        let supply = Supply::everywhere(map.size());
        World::with_supply(map, supply)
    }

    /// A world where only what `map`'s wiring and pipes reach works, as in a
    /// game.
    pub fn as_built(map: Map) -> World {
        let supply = Supply::from_map(&map);
        World::with_supply(map, supply)
    }

    fn with_supply(map: Map, supply: Supply) -> World {
        World {
            occupancy: Occupancy::new(map.size()),
            facilities: Facilities::new(&map, &supply),
            map,
            log: Log::new(),
            tick: 0,
            dt: 1.0 / 64.0,
            clock: Clock::after_watching(0.0),
            lines: Vec::new(),
        }
    }

    pub fn ctx(&self) -> Think<'_> {
        Think::new(&self.map, &self.occupancy, &self.log, &self.facilities, self.dt, self.tick, self.clock)
    }

    /// One tick for `entity`: think, the move step's rules, react.
    pub fn step(&mut self, entity: &mut dyn GameEntity) {
        self.tick += 1;
        let uid = entity.uid();
        let from = entity.center_position();
        let _ = self.occupancy.claim(from, uid);

        let intent = entity.think(&self.ctx());
        let outcome = match intent {
            Intent::Idle => MoveOutcome::Idle,
            Intent::Move { to } => {
                let into = cell_of(to);
                if into == from {
                    entity.apply(&intent);
                    MoveOutcome::Moved
                } else if uid.kind().is_some_and(EntityType::keeps_off_the_site) && self.map.site().contains(into) {
                    // `sim::move_step`'s refusal of a deathdog's step onto
                    // the site, kept in step with it.
                    MoveOutcome::Blocked { by: None }
                } else if !self.map.is_passable(into) && !self.features.is_enterable(into) {
                    // Kept in step with `sim::move_step`'s own exception for
                    // an `Access::Entered` feature — see that function's
                    // docs for why the cell is let through to the claim
                    // below rather than refused outright.
                    MoveOutcome::Blocked { by: None }
                } else if self.doors.is_shut(into) {
                    MoveOutcome::Blocked { by: None }
                } else if let Err(other) = self.occupancy.claim(into, uid) {
                    MoveOutcome::Blocked { by: Some(other) }
                } else {
                    self.occupancy.release(from, uid);
                    entity.apply(&intent);
                    MoveOutcome::Moved
                }
            }
        };
        // Seen panicking or not, as the move step would write it for a
        // crowd; anybody else a test says is panicking stays so.
        let (uid, panicking) = (entity.uid(), entity.body().is_panicking());
        self.facilities.panics.set(uid, panicking && intent != Intent::Idle);
        // In a chat or not, as the think step would gather it for a crowd;
        // anybody else a test says is chatting stays so.
        let away = entity.is_frozen() || entity.is_dead();
        let chatter = if away { None } else { entity.chatter() };
        self.facilities.chats.set(uid, chatter);
        let smalltalker = if away { None } else { entity.smalltalker() };
        self.facilities.smalltalks.set(uid, smalltalker);
        let effect = entity.react(&self.ctx(), outcome);
        self.facilities.economy.roll(self.clock.day());
        self.facilities.playzones.begin_tick();
        self.facilities.wounds.begin_tick();
        super::effect::apply(effect, entity, &mut self.map, &mut self.facilities, &mut self.occupancy, &self.log);
        self.facilities.doors.advance(self.dt, &self.occupancy);
        self.facilities.fridges.advance(self.dt * super::clock::TIME_SCALE);
        self.lines.extend(self.log.drain());
    }

    /// [`World::step`], with the clock moving on by the tick, as a
    /// `GameState`'s does. `step` holds it where a test put it, which is what
    /// a test about one moment of the day wants; this is for one about the
    /// day going by — a shift starting and ending.
    pub fn step_in_time(&mut self, entity: &mut dyn GameEntity) {
        self.step(entity);
        let watched = self.clock.elapsed() / super::clock::TIME_SCALE as f64;
        self.clock = Clock::after_watching(watched + self.dt as f64);
    }

    /// Everything the log has said, oldest first.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn log_contains(&self, needle: &str) -> bool {
        self.lines.iter().any(|line| line.contains(needle))
    }

    /// How many meals have been eaten here.
    pub fn meals(&self) -> usize {
        self.count("ate at the fridge")
    }

    /// How many drinks have been drunk here.
    pub fn drinks(&self) -> usize {
        self.count("drank at the fridge")
    }

    /// How many times a toilet has been used here.
    pub fn reliefs(&self) -> usize {
        self.count("used the toilet")
    }

    /// How many goes on a computer have been had here.
    pub fn plays(&self) -> usize {
        self.count("had a go on the computer")
    }

    /// How many games on an arcade cabinet have been played here.
    pub fn arcade_games(&self) -> usize {
        self.count("played the arcade cabinet")
    }

    /// How many hours have been slept in a bed here.
    pub fn sleeps(&self) -> usize {
        self.count("slept in the bed")
    }

    /// How many hours have been worked at a desk here.
    pub fn works(&self) -> usize {
        self.count("worked an hour at the computer")
    }

    /// How many packfuls of scrap have been stored in a bin here.
    pub fn deposits(&self) -> usize {
        self.count("scrap metal in the bin at")
    }

    fn count(&self, needle: &str) -> usize {
        self.lines.iter().filter(|line| line.contains(needle)).count()
    }
}

/// Put the prop called `name` in the middle of `cell`, where the editor would
/// have — a map's objects are positioned in pixels, not cells.
pub fn prop_at(map: &mut Map, name: &str, cell: Point) {
    map.add_object(
        ObjectLayer::Props,
        Object {
            at: Point::new(
                cell.x * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
                cell.y * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
            ),
            kind: ObjectKind::new(name),
        },
    );
}

/// A single home occupying the interior, for tests of home entertainment.
pub fn home_map(size: crate::map::Size) -> Map {
    let mut map = Map::new(size, crate::map::FLOOR);
    for y in 0..size.height {
        for x in 0..size.width {
            if x == 0 || y == 0 || x == size.width - 1 || y == size.height - 1 {
                map.set_terrain(Point::new(x, y), crate::map::WALL);
            }
        }
    }
    let door = Point::new(1, 0);
    map.set_terrain(door, crate::map::FLOOR);
    prop_at(&mut map, super::door::HOUSE_DOOR, door);
    map
}

/// A human standing in `cell` with exactly these needs, so a test decides
/// which of them are pressing rather than the spawn roll.
///
/// Fun and stamina are set to the top as well, though they are not arguments:
/// a rolled one would leave every test about hunger with a human that might
/// wander off looking for something to do, or go to bed, halfway through. A
/// test about boredom sets fun itself (`Human::set_fun`), and one about
/// sleeping sets stamina (`Human::set_stamina`).
pub fn needy_human(cell: Point, hunger: f32, thirst: f32, bladder: f32) -> Human {
    let mut rng = SmallRng::seed_from_u64(3);
    let mut human = Human::new(Uid::new(EntityType::Human, 77), cell, &mut rng);
    human.set_hunger(hunger);
    human.set_thirst(thirst);
    human.set_bladder(bladder);
    human.set_fun(100.0);
    human.set_stamina(100.0);
    human
}

/// Move `state`'s clock on to `hours` o'clock on day `day` (day 1 is a
/// Monday), as if the time between had gone by with nothing happening.
pub fn skip_to(state: &mut super::GameState, day: u32, hours: f32) {
    use super::clock::{DAY, HOUR, OPENING_TIME, TIME_SCALE};
    let world = (day - 1) as f64 * DAY as f64 + hours as f64 * HOUR as f64 - OPENING_TIME;
    assert!(world >= state.clock().elapsed(), "the clock only goes forward");
    state.elapsed = world / TIME_SCALE as f64;
}

/// A farm on good soil: a 24x14 site of meadow with a grain bin at (1, 7),
/// and a farmer hired to it with every need off. Monday, 08:00.
pub fn farm(seed: u64) -> (super::GameState, Uid) {
    let mut map = Map::new(crate::map::Size::new(24, 14), crate::map::MEADOW);
    prop_at(&mut map, "grain bin", Point::new(1, 7));
    let mut state = super::GameState::new(map, seed);
    let farmer = state.spawn(EntityType::Human, Point::new(12, 3));
    assert_eq!(state.entities().get(farmer).unwrap().job().map(|job| job.kind), Some(super::JobKind::Farmer));
    let mut input = super::Input::new();
    for process in super::ProcessId::ALL {
        input.set_process(farmer, process, false);
    }
    super::process_game_state(&mut state, 1.0 / 60.0, &input);
    (state, farmer)
}

/// A warehouse at one end of a 24x17 floor — a scrap bin with `scrap`
/// pieces in it and a steel rack — and a steel factory at the other: a
/// hopper, a pallet and a powered furnace. A steelworker and a courier
/// hired to them, every need off; the collector the bin was offered to
/// first gone again, so nobody else is working. Monday, 08:00.
pub fn steel_works(scrap: u32, seed: u64) -> (super::GameState, Uid, Uid) {
    let mut map = Map::new(crate::map::Size::new(24, 17), crate::map::FLOOR);
    for (name, cell) in [
        ("scrap bin", Point::new(2, 2)),
        ("steel rack", Point::new(2, 8)),
        ("scrap hopper", Point::new(16, 2)),
        ("steel pallet", Point::new(16, 8)),
        ("furnace", Point::new(21, 5)),
    ] {
        prop_at(&mut map, name, cell);
    }
    crate::map::utilities::serve_everything(&mut map, Point::new(0, 16), Point::new(23, 16));
    let mut state = super::GameState::new(map, seed);
    let collector = state.spawn(EntityType::Human, Point::new(8, 12));
    let smelter = state.spawn(EntityType::Human, Point::new(10, 12));
    let courier = state.spawn(EntityType::Human, Point::new(12, 12));
    let kind = |state: &super::GameState, uid: Uid| state.entities().get(uid).unwrap().job().map(|job| job.kind);
    assert_eq!(kind(&state, collector), Some(super::JobKind::ScrapCollector), "the bin is offered first");
    assert_eq!(kind(&state, smelter), Some(super::JobKind::Steelworker));
    assert_eq!(kind(&state, courier), Some(super::JobKind::Courier));
    let mut input = super::Input::new();
    input.despawn(collector);
    for uid in [smelter, courier] {
        for process in super::ProcessId::ALL {
            input.set_process(uid, process, false);
        }
    }
    super::process_game_state(&mut state, 1.0 / 60.0, &input);
    let _ = state.facilities.bins.add(Point::new(2, 2), scrap);
    (state, smelter, courier)
}

/// A farm's grain bin, powered food factory and warehouse, with workers
/// hired through the normal job allocator. The farmer is removed so input
/// and output can be accounted for exactly.
pub fn food_works(wheat: u32, seed: u64) -> (super::GameState, Uid, Uid) {
    let mut map = Map::new(crate::map::Size::new(24, 17), crate::map::FLOOR);
    for (name, cell) in [
        ("grain bin", Point::new(2, 2)),
        ("meal rack", Point::new(2, 8)),
        ("wheat hopper", Point::new(16, 2)),
        ("meal pallet", Point::new(16, 8)),
        ("food cooker", Point::new(21, 5)),
    ] {
        prop_at(&mut map, name, cell);
    }
    crate::map::utilities::serve_everything(&mut map, Point::new(0, 16), Point::new(23, 16));
    let mut state = super::GameState::new(map, seed);
    let farmer = state.spawn(EntityType::Human, Point::new(8, 12));
    let worker = state.spawn(EntityType::Human, Point::new(10, 12));
    let courier = state.spawn(EntityType::Human, Point::new(12, 12));
    assert_eq!(state.entities().get(worker).unwrap().job().unwrap().kind, super::JobKind::FoodFactoryWorker);
    assert_eq!(state.entities().get(courier).unwrap().job().unwrap().kind, super::JobKind::FoodCourier);
    let mut input = super::Input::new();
    input.despawn(farmer);
    for uid in [worker, courier] {
        for process in super::ProcessId::ALL {
            input.set_process(uid, process, false);
        }
    }
    super::process_game_state(&mut state, 1.0 / 60.0, &input);
    let _ = state.facilities.bins.add(Point::new(2, 2), wheat);
    (state, worker, courier)
}
