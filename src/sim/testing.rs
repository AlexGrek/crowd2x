//! A world of one, for tests about what a single unit decides.
//!
//! `GameState` cannot hand a test the `Human` behind a `dyn GameEntity` — the
//! entity table is deliberately not downcastable — so a test that needs to set
//! somebody's hunger and then watch them runs them here instead: the same
//! three steps as `process_pass`, over one entity, with the move step's rules
//! applied by hand.

use crate::map::Map;

use super::entity::{cell_of, GameEntity, Think};
use super::feature::Features;
use super::log::Log;
use super::occupancy::Occupancy;
use super::{Intent, MoveOutcome};

pub struct World {
    pub map: Map,
    pub occupancy: Occupancy,
    pub log: Log,
    pub features: Features,
    pub tick: u64,
    pub dt: f32,
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
                } else if !self.map.is_passable(into) {
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

    pub fn log_contains(&self, needle: &str) -> bool {
        self.lines.iter().any(|line| line.contains(needle))
    }

    /// How many meals have been eaten here.
    pub fn meals(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| line.contains("ate at the fridge"))
            .count()
    }
}
