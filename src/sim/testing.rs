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
use super::feature::Features;
use super::bins::Bins;
use super::corpses::Corpses;
use super::homes::Homes;
use super::job::Workplaces;
use super::fridge::Fridges;
use super::door::Doors;
use super::property::Properties;
use super::kinds::Human;
use super::log::Log;
use super::occupancy::Occupancy;
use super::uid::{EntityType, Uid};
use super::{Effect, Intent, MoveOutcome};

pub struct World {
    pub map: Map,
    pub occupancy: Occupancy,
    pub log: Log,
    pub features: Features,
    /// Every fridge's door and temperature. Built from `map` once, like
    /// `features`; [`World::step`] is what applies a react'd
    /// [`Effect::Fridge`] and advances it, the same as
    /// [`super::world_step`] does in a real tick.
    pub fridges: Fridges,
    /// What every scrap bin holds. Built from `map` once; [`World::step`]
    /// applies an [`Effect::Deposit`] to it, as [`super::world_step`] does.
    pub bins: Bins,
    /// Every door, and the properties the house doors lock. Built from `map`
    /// once; [`World::step`] applies an [`Effect::Door`] and moves the
    /// leaves on, as [`super::world_step`] does.
    pub doors: Doors,
    pub properties: Properties,
    /// Who lies dead where, filed and taken off by [`World::step`] as
    /// [`super::world_step`] does; put a body in it to give an undertaker
    /// one to find.
    pub corpses: Corpses,
    /// The bodies taken up, which a real tick would take out of the world.
    pub burials: Vec<Uid>,
    homes: Homes,
    workplaces: Workplaces,
    pub tick: u64,
    pub dt: f32,
    /// What time it is. Opening time, until a test sets it: a test about
    /// sleeping at night puts it there.
    pub clock: Clock,
    /// Everything the log has said, kept: the log itself is drained.
    lines: Vec<String>,
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
        let properties = Properties::from_map(&map);
        World {
            occupancy: Occupancy::new(map.size()),
            features: Features::from_map(&map, &supply),
            fridges: Fridges::from_map(&map, &supply),
            bins: Bins::from_map(&map),
            doors: Doors::from_map(&map, &properties),
            properties,
            corpses: Corpses::new(),
            burials: Vec::new(),
            homes: Homes::new(),
            workplaces: Workplaces::new(),
            map,
            log: Log::new(),
            tick: 0,
            dt: 1.0 / 64.0,
            clock: Clock::after_watching(0.0),
            lines: Vec::new(),
        }
    }

    pub fn ctx(&self) -> Think<'_> {
        Think {
            map: &self.map,
            occupancy: &self.occupancy,
            log: &self.log,
            features: &self.features,
            fridges: &self.fridges,
            doors: &self.doors,
            properties: &self.properties,
            corpses: &self.corpses,
            dt: self.dt,
            tick: self.tick,
            clock: self.clock,
        }
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
        let effect = entity.react(&self.ctx(), outcome);
        match effect {
            Effect::Fridge { at, open } => {
                let _ = self.fridges.set_open(at, open);
            }
            Effect::Door { at, key } => {
                let _ = self.doors.want(at, key);
            }
            Effect::Salvage { at } => super::salvage(entity, &mut self.map, &self.log, at),
            Effect::Deposit { at, pieces } => super::deposit(entity.uid(), &mut self.bins, &self.log, at, pieces),
            Effect::Died { cause } => {
                let log = Log::new();
                super::died(entity, &mut self.people(), &log, cause);
                self.lines.extend(log.drain());
            }
            Effect::TakeBody { at } => {
                let log = Log::new();
                super::take_body(entity, &mut self.people(), &log, at);
                self.lines.extend(log.drain());
            }
            Effect::Cremate { at } => {
                self.log.push(format!("{} cremated a body in the crematorium at {}, {}", entity.uid(), at.x, at.y));
            }
            Effect::None => {}
        }
        self.doors.advance(self.dt, &self.occupancy);
        self.fridges.advance(self.dt * super::clock::TIME_SCALE);
        self.lines.extend(self.log.drain());
    }

    fn people(&mut self) -> super::People<'_> {
        super::People {
            occupancy: &mut self.occupancy,
            homes: &mut self.homes,
            workplaces: &mut self.workplaces,
            corpses: &mut self.corpses,
            burials: &mut self.burials,
        }
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
