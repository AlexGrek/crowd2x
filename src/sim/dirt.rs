//! Four independent surface contaminants, on bodies and on the ground.
//!
//! Values are fractions (0 = clean, 1 = saturated). A successful step into a
//! new cell exchanges a quarter of each difference between the body and the
//! ground ([`Dirt::exchange`]), conserving each component — all but a trace
//! too faint to draw ([`TRACE`]), which is let go, so a trail fades out where
//! it would otherwise thin for ever and a chunk walked clean again can be
//! reclaimed.
//!
//! Where it comes from:
//!
//! * **Work in the earth and the scrap.** A finished piece of sowing,
//!   tending, harvesting, planting flowers or salvaging soils whoever did it
//!   ([`Dirt::soil`] with [`SOWING`], [`TENDING`], [`HARVESTING`],
//!   [`PLANTING`], [`SALVAGING`]), and it goes where they walk.
//! * **Puddles** in the wasteland (`map::wasteland::Wasteland::puddle`),
//!   which are constants: a step into one takes the body a quarter of the
//!   way to the puddle, and the puddle is exactly as dirty as it was. Nothing
//!   writes one and nothing saves one; it is a pure function of the seed, as
//!   the ground under it is.

use crate::map::chunk::{CHUNK_AREA, ChunkGrid, ChunkPos, local_index};
use crate::map::wasteland::Wasteland;
use crate::map::{Bounds, Map, Point, Size};
use crate::save::{Invalid, invalid, pb};

pub const EXCHANGE: f32 = 0.25;

/// A component under this is nothing: too faint to draw (the game draws in
/// sixty-fourths, rounded), and let go by an exchange rather than spread
/// thinner for ever.
pub const TRACE: f32 = 1.0 / 256.0;

// What a finished piece of work leaves on whoever did it (`Dirt::soil`): the
// share of what was still clean that it covers. A farmer's ten crops tended
// send it home filthy; a piece cut short leaves nothing.
/// Ten minutes' sowing, hands in the soil.
pub const SOWING: Dirt = Dirt::earth(0.2);
/// A quarter of an hour weeding and watering a crop.
pub const TENDING: Dirt = Dirt::earth(0.1);
/// Cutting a ripe crop.
pub const HARVESTING: Dirt = Dirt::earth(0.15);
/// Planting a flower bed.
pub const PLANTING: Dirt = Dirt::earth(0.2);
/// Working a piece of scrap metal loose from a heap.
pub const SALVAGING: Dirt = Dirt::earth(0.15);

/// A bladder accident leaves a wet body and a wet patch underfoot.
pub const PEE: Dirt = Dirt { wet: 1.0, ..Dirt::CLEAN };

/// Wetness lost on each whole world hour: a saturated patch dries in four.
pub const EVAPORATION_PER_HOUR: f32 = 0.25;

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
        let map = Map::new(Size::new(2, 2), FLOOR);
        let cell = Point::new(-33, 77);
        let mut layer = DirtLayer::new(&map);
        layer.prepare(ChunkPos::of(cell));
        assert!(layer.set(cell, STAIN));
        let mut unit = Dirt::CLEAN;
        layer.enter(cell, &mut unit);
        layer.sweep(&mut 0, 100, |_| false);
        assert_eq!(layer.at(cell).dirt, 0.75);
        let saved = layer.save();
        let restored = DirtLayer::restore(&map, &saved).unwrap();
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
        assert!(DirtLayer::restore(&map, &[saved[0], saved[0]]).is_err());
        assert_eq!(layer.at(Point::new(1000, -1000)), Dirt::CLEAN);
    }

    #[test]
    fn a_trace_too_faint_to_draw_is_let_go_so_a_trail_ends() {
        let mut unit = Dirt::earth(0.005);
        let mut tile = Dirt::CLEAN;
        unit.exchange(&mut tile);
        assert_eq!((unit, tile), (Dirt::CLEAN, Dirt::CLEAN), "both under a trace");
        // Filthy feet over clean ground: a trail a dozen or so cells long,
        // then nothing, rather than a cell's worth less each step for ever.
        let map = Map::new(Size::new(64, 1), FLOOR);
        let mut layer = DirtLayer::new(&map);
        let mut feet = Dirt::earth(1.0);
        for x in 0..64 {
            layer.enter(Point::new(x, 0), &mut feet);
        }
        let trail = (0..64).filter(|&x| layer.at(Point::new(x, 0)) != Dirt::CLEAN).count();
        assert!((10..25).contains(&trail), "a trail {trail} cells long");
        assert_eq!(feet, Dirt::CLEAN);
        assert!(layer.at(Point::new(trail as i32 - 1, 0)).dirt >= TRACE);
    }

    #[test]
    fn work_soils_up_to_saturated_and_never_cleans() {
        let mut body = Dirt { blood: 0.5, ..Dirt::CLEAN };
        body.soil(SOWING);
        assert_eq!(body, Dirt { dirt: 0.2, blood: 0.5, ..Dirt::CLEAN });
        let mut last = body.dirt;
        for _ in 0..200 {
            body.soil(SALVAGING);
            assert!(body.is_valid() && body.dirt >= last && body.blood == 0.5);
            last = body.dirt;
        }
        assert!(body.dirt > 0.99);
        for grime in [SOWING, TENDING, HARVESTING, PLANTING, SALVAGING] {
            assert!(grime.is_valid() && grime.dirt > 0.0, "{grime:?}");
        }
    }

    /// A small endless site and a puddle in the column just off its left
    /// edge, with the cell beyond it to walk in from.
    fn puddle_beside_a_site() -> (Map, Point) {
        for seed in 0..500 {
            let map = Map::new(Size::new(4, 4), FLOOR).with_wasteland(seed);
            let layer = DirtLayer::new(&map);
            for y in 0..4 {
                let puddle = Point::new(-1, y);
                if layer.puddle(puddle).is_some() && map.is_passable(puddle.offset(-1, 0)) {
                    return (map, puddle);
                }
            }
        }
        panic!("no puddle beside the site in 500 seeds");
    }

    #[test]
    fn a_puddle_soils_whoever_walks_through_it_and_stays_as_dirty_as_it_was() {
        let (map, cell) = puddle_beside_a_site();
        let mut layer = DirtLayer::new(&map);
        let puddle = layer.puddle(cell).unwrap();
        assert!(puddle.dirt >= 0.5 && puddle == Dirt::earth(puddle.dirt));
        assert_eq!(layer.at(cell), puddle);
        let mut feet = Dirt { blood: 0.5, ..Dirt::CLEAN };
        for step in 1..=40 {
            layer.enter(cell, &mut feet);
            assert_eq!(layer.at(cell), puddle, "after {step} steps through it");
            assert!(feet.dirt <= puddle.dirt);
        }
        assert!(feet.dirt > puddle.dirt * 0.99, "{feet:?} after forty steps");
        assert_eq!(feet.blood, 0.0, "a puddle washes off what it is clean of");
        assert!(layer.set(cell, STAIN), "a valid vector is accepted");
        assert_eq!(layer.at(cell), puddle, "and a puddle is not changed by it");
        assert!(layer.save().is_empty(), "nothing about a puddle is stored");
        assert_eq!(DirtLayer::new(&Map::new(Size::new(4, 4), FLOOR)).puddle(cell), None, "no wasteland, no puddles");
        assert!(map.site().points().all(|on| layer.puddle(on).is_none()));
    }

    #[test]
    fn a_body_that_walks_through_a_puddle_tracks_it_onto_the_site() {
        let (map, puddle) = puddle_beside_a_site();
        let mut state = GameState::new(map, 7);
        let uid = state.spawn(EntityType::Human, puddle.offset(-1, 0));
        spawn_pass(&mut state, &Input::new());
        let middle = |cell: Point| (cell.x as f32 + 0.5, cell.y as f32 + 0.5);
        step(&mut state, uid, middle(puddle));
        let soiled = state.entities().get(uid).unwrap().body().dirt();
        assert_eq!(soiled.dirt, state.dirt().at(puddle).dirt * EXCHANGE);
        step(&mut state, uid, middle(Point::new(0, puddle.y)));
        assert_eq!(state.dirt().at(Point::new(0, puddle.y)).dirt, soiled.dirt * EXCHANGE);
        assert_eq!(state.dirt().puddle(puddle).unwrap(), state.dirt().at(puddle));
    }
}

impl Dirt {
    pub const CLEAN: Self = Self {
        dirt: 0.0,
        blood: 0.0,
        wet: 0.0,
        burns: 0.0,
    };

    /// Earth and nothing else, this much of it: a puddle, or a piece of
    /// work's grime.
    pub const fn earth(dirt: f32) -> Self {
        Self { dirt, ..Self::CLEAN }
    }

    pub fn components(self) -> [f32; 4] {
        [self.dirt, self.blood, self.wet, self.burns]
    }

    /// How dirty, all told: the four components summed. What a cleaner asks
    /// of a cell ([`crate::sim::brain::goals::clean::DIRTY`]).
    pub fn overall(self) -> f32 {
        self.components().into_iter().sum()
    }

    /// What is on it, for a log line: `"dirt and blood"`, or `"nothing"`.
    pub fn describe(self) -> String {
        const NAMES: [&str; 4] = ["dirt", "blood", "wet", "burns"];
        let present: Vec<&str> =
            NAMES.into_iter().zip(self.components()).filter(|&(_, v)| v > 0.0).map(|(name, _)| name).collect();
        match present.as_slice() {
            [] => "nothing".to_string(),
            [only] => only.to_string(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        }
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
            *carried = faint(*carried + transfer);
            *surface = faint(*surface - transfer);
        }
    }

    /// Soiled by work: each component covers `grime`'s share of what of it
    /// was still clean. Never cleaner for it, and never past saturated.
    pub fn soil(&mut self, grime: Dirt) {
        for (carried, added) in [
            (&mut self.dirt, grime.dirt),
            (&mut self.blood, grime.blood),
            (&mut self.wet, grime.wet),
            (&mut self.burns, grime.burns),
        ] {
            *carried += (1.0 - *carried) * added;
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

/// A [`TRACE`] or less is nothing.
fn faint(value: f32) -> f32 {
    if value < TRACE { 0.0 } else { value }
}

/// Dense on the site; chunked outside it. Clean, absent chunks read as zero.
/// The spawn pass prepares chunks around the crowd, as occupancy does. Dirty
/// chunks persist when nobody is nearby: footprints must survive a return trip.
/// The wasteland's puddles are read off its seed, never stored.
pub struct DirtLayer {
    size: Size,
    cells: Vec<Dirt>,
    wild: ChunkGrid<Box<[Dirt; CHUNK_AREA]>>,
    /// The ground round the site, for its puddles: `None` round a map with
    /// nothing round it.
    wasteland: Option<Wasteland>,
}

impl Default for DirtLayer {
    fn default() -> Self {
        Self::of(Size::new(1, 1), None)
    }
}

impl DirtLayer {
    /// The hourly world process visits all stored cells, including distant
    /// wasteland chunks. Earth, blood and burns are never evaporated.
    pub(crate) fn evaporate(&mut self, hours: u64) {
        if hours == 0 { return; }
        let amount = EVAPORATION_PER_HOUR * hours as f32;
        for value in &mut self.cells {
            value.wet = faint((value.wet - amount).max(0.0));
        }
        for (_, chunk) in self.wild.iter_mut() {
            for value in chunk.iter_mut() {
                value.wet = faint((value.wet - amount).max(0.0));
            }
        }
    }

    /// Clean ground on `map`'s site, and its wasteland's puddles round it.
    pub fn new(map: &Map) -> Self {
        Self::of(map.size(), map.wasteland())
    }

    fn of(size: Size, wasteland: Option<Wasteland>) -> Self {
        Self {
            size,
            cells: vec![Dirt::CLEAN; size.area()],
            wild: ChunkGrid::default(),
            wasteland,
        }
    }

    /// Every stained cell of the site, row by row: a read of the dense
    /// layer and no more, which is what a cleaner's look over the district
    /// is (`goals::clean`). The wasteland's chunks are not in it.
    pub fn site_stains(&self) -> impl Iterator<Item = (Point, Dirt)> + '_ {
        self.size.points().zip(self.cells.iter().copied()).filter(|&(_, value)| value != Dirt::CLEAN)
    }

    pub fn at(&self, cell: Point) -> Dirt {
        if let Some(index) = self.size.index_of(cell) {
            return self.cells[index];
        }
        self.puddle(cell).unwrap_or_else(|| self.wild_at(cell))
    }

    /// The puddle in `cell`, if there is one: off the site, out of the seed.
    pub fn puddle(&self, cell: Point) -> Option<Dirt> {
        let dirt = self.wasteland?.puddle(cell, Bounds::from(self.size))?;
        Some(Dirt::earth(dirt))
    }

    /// What is stored for a cell off the site — under a puddle, nothing.
    fn wild_at(&self, cell: Point) -> Dirt {
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

    /// Lay `value` on `cell`, refusing what [`Dirt::is_valid`] refuses. A
    /// puddle is a constant: setting one is accepted and changes nothing.
    pub fn set(&mut self, cell: Point, value: Dirt) -> bool {
        if !value.is_valid() {
            return false;
        }
        if self.size.index_of(cell).is_none() && self.puddle(cell).is_some() {
            return true;
        }
        if self.at(cell) != value {
            *self.at_mut(cell) = value;
        }
        true
    }

    /// Add a stain without cleaning any existing component. Wasteland
    /// puddles retain their constant, seed-derived contamination.
    pub(crate) fn soil(&mut self, cell: Point, grime: Dirt) {
        let mut value = self.at(cell);
        value.soil(grime);
        let _ = self.set(cell, value);
    }

    /// A body carrying `carried` has stepped into `cell`. A puddle is
    /// exchanged with like any ground, and is none the worse or better for it.
    pub fn enter(&mut self, cell: Point, carried: &mut Dirt) {
        let ground = match self.size.index_of(cell) {
            Some(index) => self.cells[index],
            None => match self.puddle(cell) {
                Some(mut puddle) => return carried.exchange(&mut puddle),
                None => self.wild_at(cell),
            },
        };
        if *carried == Dirt::CLEAN && ground == Dirt::CLEAN {
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

    pub(crate) fn restore(map: &Map, saved: &[pb::DirtCell]) -> Result<Self, Invalid> {
        let mut layer = Self::new(map);
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
