//! Four independent surface contaminants, on bodies and on the ground.
//! Values are fractions (0 = clean, 1 = saturated). A successful cell entry
//! exchanges a quarter of each difference, conserving each component.

use crate::map::chunk::{CHUNK_AREA, ChunkGrid, ChunkPos, local_index};
use crate::map::{Point, Size};
use crate::save::{Invalid, invalid, pb};

pub const EXCHANGE: f32 = 0.25;

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dirt {
    pub dirt: f32,
    pub blood: f32,
    pub wet: f32,
    pub burns: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Map, WALL};
    use crate::sim::{EntityType, GameState, Input, Intent, MoveOutcome, spawn_pass};

    const STAIN: Dirt = Dirt {
        dirt: 1.0,
        blood: 0.5,
        wet: 0.25,
        burns: 0.75,
    };

    fn step(state: &mut GameState, uid: crate::sim::Uid, to: (f32, f32)) {
        state.intents.fill(Intent::Idle);
        let slot = state.entities.slot_of(uid).unwrap() as usize;
        state.intents[slot] = Intent::Move { to };
        let f = &mut state.facilities;
        crate::sim::move_step(
            &mut state.entities.freeze(),
            &state.map,
            &mut state.occupancy,
            &mut state.census,
            &f.features,
            &f.doors,
            &mut f.panics,
            &mut f.dirt,
            &state.intents,
            &mut state.moves,
        );
    }

    #[test]
    fn exchange_moves_each_component_both_ways_and_conserves_it() {
        let mut unit = Dirt {
            dirt: 0.0,
            blood: 1.0,
            wet: 0.25,
            burns: 0.75,
        };
        let mut tile = Dirt {
            dirt: 1.0,
            blood: 0.0,
            wet: 0.75,
            burns: 0.25,
        };
        unit.exchange(&mut tile);
        assert_eq!(
            unit,
            Dirt {
                dirt: 0.25,
                blood: 0.75,
                wet: 0.375,
                burns: 0.625
            }
        );
        for (a, b) in unit.components().into_iter().zip(tile.components()) {
            assert_eq!(a + b, 1.0);
        }
        for _ in 0..100 {
            unit.exchange(&mut tile);
            assert!(unit.is_valid() && tile.is_valid());
        }
    }

    #[test]
    fn successful_entry_picks_up_then_spreads_but_subcell_steps_and_blocks_do_not() {
        let mut map = Map::new(Size::new(5, 2), FLOOR);
        map.set_terrain(Point::new(3, 0), WALL);
        let mut state = GameState::new(map, 7);
        let uid = state.spawn(EntityType::Human, Point::new(0, 0));
        let other = state.spawn(EntityType::Dog, Point::new(2, 1));
        let mut input = Input::new();
        input.set_dirt(Point::new(1, 0), STAIN);
        spawn_pass(&mut state, &input);
        step(&mut state, uid, (1.1, 0.5));
        let carried = state.entities().get(uid).unwrap().body().dirt();
        assert_eq!(carried.dirt, 0.25);
        assert_eq!(state.dirt().at(Point::new(1, 0)).dirt, 0.75);
        // More movement within the tile and idling never exchange twice.
        step(&mut state, uid, (1.9, 0.5));
        assert_eq!(state.entities().get(uid).unwrap().body().dirt(), carried);
        step(&mut state, uid, (2.1, 0.5));
        assert_eq!(state.dirt().at(Point::new(2, 0)).dirt, 0.0625);
        let before = state.entities().get(uid).unwrap().body().dirt();
        for to in [(3.1, 0.5), (2.1, 1.5), (-1.0, 0.5)] {
            step(&mut state, uid, to);
            assert!(matches!(
                state.last_move(uid),
                Some(MoveOutcome::Blocked { .. })
            ));
            assert_eq!(state.entities().get(uid).unwrap().body().dirt(), before);
            assert_eq!(
                state.dirt().at(crate::sim::entity::cell_of(to)),
                Dirt::CLEAN
            );
        }
        // A second body picks up the trail left on an initially clean tile.
        step(&mut state, uid, (1.5, 0.5));
        step(&mut state, other, (2.5, 0.5));
        assert_eq!(
            state.entities().get(other).unwrap().body().dirt().dirt,
            0.015625
        );
    }

    #[test]
    fn underfoot_reads_current_ground_even_when_facing_away() {
        let mut world = crate::sim::testing::World::new(Map::new(Size::new(3, 3), FLOOR));
        let cell = Point::new(1, 1);
        world.facilities.dirt.set(cell, STAIN);
        let body =
            crate::sim::entity::Body::at_cell(crate::sim::Uid::new(EntityType::Dog, 1), cell);
        let perception = crate::sim::brain::perception::Perception::default();
        assert_eq!(perception.underfoot(&world.ctx(), &body), STAIN);
        world.facilities.dirt.set(cell, Dirt::CLEAN);
        assert_eq!(perception.underfoot(&world.ctx(), &body), Dirt::CLEAN);
    }

    #[test]
    fn wasteland_stains_survive_reclaim_and_round_trip_and_bad_values_are_refused() {
        let size = Size::new(2, 2);
        let cell = Point::new(-33, 77);
        let mut layer = DirtLayer::new(size);
        layer.prepare(ChunkPos::of(cell));
        assert!(layer.set(cell, STAIN));
        let mut unit = Dirt::CLEAN;
        layer.enter(cell, &mut unit);
        layer.sweep(&mut 0, 100, |_| false);
        assert_eq!(layer.at(cell).dirt, 0.75);
        let saved = layer.save();
        let restored = DirtLayer::restore(size, &saved).unwrap();
        assert_eq!(saved, restored.save());
        for v in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(!layer.set(cell, Dirt { blood: v, ..STAIN }));
            assert!(
                Dirt::restore(&pb::Dirt {
                    wet: v,
                    ..Default::default()
                })
                .is_err()
            );
        }
        assert!(DirtLayer::restore(size, &[saved[0], saved[0]]).is_err());
        assert_eq!(layer.at(Point::new(1000, -1000)), Dirt::CLEAN);
    }
}

impl Dirt {
    pub const CLEAN: Self = Self {
        dirt: 0.0,
        blood: 0.0,
        wet: 0.0,
        burns: 0.0,
    };

    pub fn components(self) -> [f32; 4] {
        [self.dirt, self.blood, self.wet, self.burns]
    }

    pub fn is_valid(self) -> bool {
        self.components()
            .into_iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
    }

    pub fn exchange(&mut self, ground: &mut Self) {
        for (carried, surface) in [
            (&mut self.dirt, &mut ground.dirt),
            (&mut self.blood, &mut ground.blood),
            (&mut self.wet, &mut ground.wet),
            (&mut self.burns, &mut ground.burns),
        ] {
            let transfer = (*surface - *carried) * EXCHANGE;
            *carried += transfer;
            *surface -= transfer;
        }
    }

    pub(crate) fn save(self) -> pb::Dirt {
        pb::Dirt {
            dirt: self.dirt,
            blood: self.blood,
            wet: self.wet,
            burns: self.burns,
        }
    }

    pub(crate) fn restore(saved: &pb::Dirt) -> Result<Self, Invalid> {
        let value = Self {
            dirt: saved.dirt,
            blood: saved.blood,
            wet: saved.wet,
            burns: saved.burns,
        };
        if !value.is_valid() {
            return invalid("dirt components must be finite fractions in 0..=1");
        }
        Ok(value)
    }
}

/// Dense on the site; chunked outside it. Clean, absent chunks read as zero.
/// The spawn pass prepares chunks around the crowd, as occupancy does. Dirty
/// chunks persist when nobody is nearby: footprints must survive a return trip.
pub struct DirtLayer {
    size: Size,
    cells: Vec<Dirt>,
    wild: ChunkGrid<Box<[Dirt; CHUNK_AREA]>>,
}

impl Default for DirtLayer {
    fn default() -> Self {
        Self::new(Size::new(1, 1))
    }
}

impl DirtLayer {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            cells: vec![Dirt::CLEAN; size.area()],
            wild: ChunkGrid::default(),
        }
    }

    pub fn at(&self, cell: Point) -> Dirt {
        if let Some(index) = self.size.index_of(cell) {
            return self.cells[index];
        }
        self.wild
            .get(ChunkPos::of(cell))
            .map_or(Dirt::CLEAN, |chunk| chunk[local_index(cell)])
    }

    fn at_mut(&mut self, cell: Point) -> &mut Dirt {
        if let Some(index) = self.size.index_of(cell) {
            return &mut self.cells[index];
        }
        &mut self
            .wild
            .get_or_insert_with(ChunkPos::of(cell), || Box::new([Dirt::CLEAN; CHUNK_AREA]))
            [local_index(cell)]
    }

    pub fn set(&mut self, cell: Point, value: Dirt) -> bool {
        if !value.is_valid() {
            return false;
        }
        if self.at(cell) != value {
            *self.at_mut(cell) = value;
        }
        true
    }

    pub fn enter(&mut self, cell: Point, carried: &mut Dirt) {
        if *carried == Dirt::CLEAN && self.at(cell) == Dirt::CLEAN {
            return;
        }
        carried.exchange(self.at_mut(cell));
    }

    pub fn prepare(&mut self, around: ChunkPos) {
        for pos in around.around(1) {
            let bounds = pos.bounds();
            if self.size.contains(bounds.origin) && self.size.contains(bounds.max()) {
                continue;
            }
            self.wild
                .get_or_insert_with(pos, || Box::new([Dirt::CLEAN; CHUNK_AREA]));
        }
    }

    pub fn sweep(
        &mut self,
        cursor: &mut usize,
        count: usize,
        mut keep: impl FnMut(ChunkPos) -> bool,
    ) {
        self.wild.sweep(cursor, count, |pos, cells| {
            keep(pos) || cells.iter().any(|v| *v != Dirt::CLEAN)
        });
    }

    pub(crate) fn save(&self) -> Vec<pb::DirtCell> {
        let mut cells: Vec<_> = self
            .size
            .points()
            .filter_map(|cell| {
                let value = self.at(cell);
                (value != Dirt::CLEAN).then_some(pb::DirtCell {
                    cell: Some(cell.into()),
                    value: Some(value.save()),
                })
            })
            .collect();
        for (pos, chunk) in self.wild.iter() {
            for cell in pos.bounds().points() {
                let value = chunk[local_index(cell)];
                if self.size.contains(cell) || value == Dirt::CLEAN {
                    continue;
                }
                cells.push(pb::DirtCell {
                    cell: Some(cell.into()),
                    value: Some(value.save()),
                });
            }
        }
        cells.sort_unstable_by_key(|cell| {
            let p = cell.cell.unwrap();
            (p.x, p.y)
        });
        cells
    }

    pub(crate) fn restore(size: Size, saved: &[pb::DirtCell]) -> Result<Self, Invalid> {
        let mut layer = Self::new(size);
        let mut seen = std::collections::BTreeSet::new();
        for entry in saved {
            let cell = pb::cell(entry.cell, "dirt")?;
            if !seen.insert(cell) {
                return invalid("duplicate dirt cell");
            }
            let value = Dirt::restore(pb::required(&entry.value, "dirt vector")?)?;
            layer.set(cell, value);
        }
        Ok(layer)
    }
}
