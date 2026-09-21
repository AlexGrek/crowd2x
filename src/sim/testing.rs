//! A world of one, for tests about what a single unit decides.
//!
//! `GameState` cannot hand a test the `Human` behind a `dyn GameEntity` — the
//! entity table is deliberately not downcastable — so a test that needs to set
//! somebody's hunger and then watch them runs them here instead: the same
//! three steps as `process_pass`, over one entity, with the move step's rules
//! applied by hand.

use rand::rngs::SmallRng;
use rand::SeedableRng;

use crate::map::{Map, Object, ObjectKind, ObjectLayer, Point, PIXELS_PER_CELL};

use super::clock::Clock;
use super::entity::{cell_of, GameEntity, Think};
use super::feature::Features;
use super::kinds::Human;
use super::log::Log;
use super::occupancy::Occupancy;
use super::uid::{EntityType, Uid};
use super::{Intent, MoveOutcome};

pub struct World {
    pub map: Map,
    pub occupancy: Occupancy,
    pub log: Log,
    pub features: Features,
    pub tick: u64,
    pub dt: f32,
    /// What time it is. Opening time, until a test sets it: a test about
    /// sleeping at night puts it there.
    pub clock: Clock,
    /// Everything the log has said, kept: the log itself is drained.
    lines: Vec<String>,
}

impl World {
    pub fn new(map: Map) -> World {
        World {
            occupancy: Occupancy::new(map.size()),
            features: Features::from_map(&map),
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
                } else if let Err(other) = self.occupancy.claim(into, uid) {
                    MoveOutcome::Blocked { by: Some(other) }
                } else {
                    self.occupancy.release(from, uid);
                    entity.apply(&intent);
                    MoveOutcome::Moved
                }
            }
        };
        entity.react(&self.ctx(), outcome);
        self.lines.extend(self.log.drain());
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
