//! Signed airborne scent, independent of walkability and vision direction.
//! Sparse cells include the wasteland. Only active scent and its one-cell
//! frontier are sent to the compute shader, every thirty world seconds.
use super::{Clock, Doors, flowers::Flowers};
use crate::map::{Map, Point};
use crate::save::{Invalid, invalid, pb};
use std::collections::{BTreeMap, BTreeSet};

pub const MINIMUM: f32 = 0.01;
pub const MAXIMUM: f32 = 10.0;
pub const STEP_SECONDS: f64 = 30.0;
pub const FLOWER_EMISSION: f32 = 1.0;
pub const TOILET_RELEASE: f32 = -6.0;
pub const DOG_RELEASE: f32 = -10.0;

/// Furniture displaces feet, not air. Walls and closed door leaves exclude
/// the field entirely; partially opening doors stay closed until fully open.
pub fn has_air(map: &Map, doors: &Doors, cell: Point) -> bool {
    map.terrain(cell)
        .is_some_and(|tile| tile.is_passable() || tile.name() == "water")
        && !doors.is_shut(cell)
}

#[derive(Default)]
pub struct Smell {
    cells: BTreeMap<Point, f32>,
    frontier: BTreeSet<Point>,
    upload: Vec<[f32; 8]>,
    output: Vec<f32>,
}

impl Smell {
    pub fn at(&self, map: &Map, doors: &Doors, cell: Point) -> f32 {
        if has_air(map, doors, cell) {
            self.cells.get(&cell).copied().unwrap_or(0.0)
        } else {
            0.0
        }
    }

    pub fn release(&mut self, map: &Map, doors: &Doors, cell: Point, amount: f32) {
        if !amount.is_finite() || !has_air(map, doors, cell) {
            return;
        }
        let value =
            (self.cells.get(&cell).copied().unwrap_or(0.0) + amount).clamp(-MAXIMUM, MAXIMUM);
        if value.abs() < MINIMUM {
            self.cells.remove(&cell);
        } else {
            self.cells.insert(cell, value);
        }
    }

    /// A closing leaf expels its cell's scent. Do this on every tick, so
    /// reopening between field steps cannot resurrect scent inside the leaf.
    pub fn close_doors(&mut self, doors: &Doors) {
        for (cell, door) in doors.iter() {
            if !door.is_open() {
                self.cells.remove(&cell);
            }
        }
    }

    pub fn advance(
        &mut self,
        map: &Map,
        doors: &Doors,
        flowers: &Flowers,
        before: Clock,
        now: Clock,
    ) {
        let steps = (now.elapsed() / STEP_SECONDS).floor() as u64
            - (before.elapsed() / STEP_SECONDS).floor() as u64;
        for _ in 0..steps {
            self.step(map, doors, flowers, now);
        }
    }

    fn step(&mut self, map: &Map, doors: &Doors, flowers: &Flowers, now: Clock) {
        self.cells.retain(|cell, _| has_air(map, doors, *cell));
        self.frontier.clear();
        for cell in self.cells.keys().copied().chain(
            flowers
                .iter()
                .filter(|bed| bed.blooming(now))
                .map(|bed| bed.cell),
        ) {
            self.frontier.insert(cell);
            self.frontier.extend(cell.cardinal_neighbours());
        }
        self.frontier.retain(|cell| has_air(map, doors, *cell));
        self.upload.clear();
        for &cell in &self.frontier {
            let mut c = [0.0; 8];
            c[0] = self.cells.get(&cell).copied().unwrap_or(0.0);
            for (i, next) in cell.cardinal_neighbours().into_iter().enumerate() {
                if has_air(map, doors, next) {
                    c[i + 1] = self.cells.get(&next).copied().unwrap_or(0.0);
                    c[5] += 1.0;
                }
            }
            c[6] = if flowers.at(cell).is_some_and(|bed| bed.blooming(now)) {
                FLOWER_EMISSION
            } else {
                0.0
            };
            self.upload.push(c);
        }
        self.output.resize(self.upload.len(), 0.0);
        crate::smell_compute::evolve(&self.upload, &mut self.output);
        self.cells.clear();
        self.cells.extend(
            self.frontier
                .iter()
                .copied()
                .zip(self.output.iter().copied())
                .filter(|(_, value)| *value != 0.0),
        );
    }

    pub(crate) fn save(&self) -> Vec<pb::SmellCell> {
        self.cells
            .iter()
            .map(|(&cell, &value)| pb::SmellCell {
                cell: Some(cell.into()),
                value,
            })
            .collect()
    }

    pub(crate) fn restore(
        saved: &[pb::SmellCell],
        map: &Map,
        doors: &Doors,
    ) -> Result<Self, Invalid> {
        let mut field = Self::default();
        for item in saved {
            let cell = pb::cell(item.cell, "smell")?;
            if !item.value.is_finite()
                || item.value.abs() > MAXIMUM
                || item.value.abs() < MINIMUM
                || !has_air(map, doors, cell)
                || field.cells.insert(cell, item.value).is_some()
            {
                return invalid("smell: invalid or duplicate air cell/value");
            }
        }
        Ok(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, TerrainId};
    use crate::sim::testing::prop_at;
    use crate::sim::{Occupancy, Properties};

    fn map() -> Map {
        Map::new(Size::new(7, 5), TerrainId::from_name("floor").unwrap())
    }

    #[test]
    fn walls_and_closed_doors_exclude_smell_but_open_doors_carry_it() {
        let mut map = map();
        for y in 0..5 {
            map.set_terrain(
                Point::new(3, y),
                TerrainId::from_name("wall brown").unwrap(),
            );
        }
        let door = Point::new(3, 2);
        map.set_terrain(door, TerrainId::from_name("floor").unwrap());
        prop_at(&mut map, "door", door);
        let mut doors = Doors::from_map(&map, &Properties::from_map(&map));
        let mut smell = Smell::default();
        smell.release(&map, &doors, Point::new(2, 2), -10.0);
        let flowers = Flowers::default();
        let now = Clock::after_watching(0.0);
        for _ in 0..10 {
            smell.step(&map, &doors, &flowers, now);
        }
        assert_eq!(smell.at(&map, &doors, Point::new(4, 2)), 0.0);
        assert_eq!(smell.at(&map, &doors, Point::new(3, 1)), 0.0);
        assert_eq!(smell.at(&map, &doors, door), 0.0);
        assert!(doors.want(door, None));
        doors.advance(super::super::door::OPENING, &Occupancy::new(map.size()));
        smell.release(&map, &doors, Point::new(2, 2), -10.0);
        for _ in 0..3 {
            smell.step(&map, &doors, &flowers, now);
        }
        assert!(smell.at(&map, &doors, door) < 0.0);
        assert!(smell.at(&map, &doors, Point::new(4, 2)) < 0.0);
        assert_eq!(smell.at(&map, &doors, Point::new(3, 1)), 0.0);
    }

    #[test]
    fn signed_float_scents_mix_clamp_and_eventually_disappear() {
        let map = map();
        let doors = Doors::default();
        let at = Point::new(2, 2);
        let mut smell = Smell::default();
        smell.release(&map, &doors, at, 30.0);
        assert_eq!(smell.at(&map, &doors, at), 10.0);
        smell.release(&map, &doors, at, -11.25);
        assert_eq!(smell.at(&map, &doors, at), -1.25);
        smell.release(&map, &doors, at, f32::NAN);
        assert_eq!(smell.at(&map, &doors, at), -1.25);
        smell.step(
            &map,
            &doors,
            &Flowers::default(),
            Clock::after_watching(0.0),
        );
        assert!(smell.at(&map, &doors, Point::new(2, 3)) < 0.0);
        for _ in 0..300 {
            smell.step(
                &map,
                &doors,
                &Flowers::default(),
                Clock::after_watching(0.0),
            );
        }
        assert!(smell.cells.is_empty());
    }

    #[test]
    fn only_blooming_flowers_emit_and_the_field_survives_saving() {
        let map = Map::new(Size::new(7, 5), TerrainId::from_name("grass").unwrap());
        let doors = Doors::default();
        let at = Point::new(2, 2);
        let mut flowers = Flowers::default();
        let planted = Clock::after_watching(0.0);
        assert!(flowers.sow(&map, at, 0, planted));
        let mut smell = Smell::default();
        smell.step(&map, &doors, &flowers, planted);
        assert_eq!(smell.at(&map, &doors, at), 0.0);
        let blooming = Clock::after_watching(
            super::super::flowers::GROW_SECONDS / super::super::clock::TIME_SCALE as f64,
        );
        smell.step(&map, &doors, &flowers, blooming);
        assert_eq!(smell.at(&map, &doors, at), FLOWER_EMISSION);
        smell.step(&map, &doors, &flowers, blooming);
        assert!(smell.at(&map, &doors, Point::new(2, 3)) > 0.0);
        let saved = smell.save();
        assert_eq!(Smell::restore(&saved, &map, &doors).unwrap().save(), saved);
        assert!(
            Smell::restore(
                &[pb::SmellCell {
                    cell: Some(at.into()),
                    value: f32::NAN
                }],
                &map,
                &doors
            )
            .is_err()
        );
        assert!(Smell::restore(&[saved[0], saved[0]], &map, &doors).is_err());
        assert!(Smell::restore(&[], &map, &doors).unwrap().cells.is_empty());
        smell.cells.clear();
        let withered = Clock::after_watching(
            super::super::flowers::WITHER_AGE / super::super::clock::TIME_SCALE as f64,
        );
        smell.step(&map, &doors, &flowers, withered);
        assert!(smell.cells.is_empty(), "withered flowers stop emitting");
    }

    #[test]
    fn smell_exists_outside_the_site_and_flows_across_its_edge() {
        let map = map().with_wasteland(5);
        let doors = Doors::default();
        let mut smell = Smell::default();
        // Find an open pair across the district boundary, independent of
        // which wasteland terrain this seed generated.
        let (inside, outside) = (0..5)
            .map(|y| (Point::new(0, y), Point::new(-1, y)))
            .find(|&(_, outside)| has_air(&map, &doors, outside))
            .unwrap();
        smell.release(&map, &doors, inside, -10.0);
        smell.step(
            &map,
            &doors,
            &Flowers::default(),
            Clock::after_watching(0.0),
        );
        assert!(smell.at(&map, &doors, outside) < 0.0);
        let saved = smell.save();
        assert_eq!(Smell::restore(&saved, &map, &doors).unwrap().save(), saved);
    }

    /// Scent moves the mind as a rate: by how strong it is and how long it is
    /// breathed — a sliver a look, where it was once a whole point a look and
    /// swung a crowd from one end to the other in a day (`biology::mind`).
    #[test]
    fn humans_breathe_scent_into_their_mind_by_its_strength_and_the_time_breathed() {
        use crate::sim::{
            GameEntity,
            background::Priority,
            biology::mind::HOURS_OF_SCENT,
            clock::{HOUR, TIME_SCALE},
            testing::{World, needy_human},
        };
        const BEATS: u32 = 4;
        let mut moved = Vec::new();
        for scent in [0.0, 0.125, 10.0, -0.125, -10.0] {
            let at = Point::new(2, 2);
            let mut map = map();
            for cell in at.cardinal_neighbours() {
                prop_at(&mut map, "crate", cell);
            }
            let mut world = World::new(map);
            let mut human = needy_human(at, 0.0, 0.0, 0.0);
            // Fun between joyless and cheerful: the air is all that moves it.
            human
                .biology_mut()
                .unwrap()
                .edit(|_| crate::sim::Stats::calm().with_fun(30.0));
            world
                .facilities
                .smell
                .release(&world.map, &world.facilities.doors, at, scent);
            let ticks = BEATS * Priority::Low.period() as u32;
            for _ in 0..ticks {
                world.step(&mut human);
            }
            let change = human.stats().mental_health() - 50.0;
            let breathed = ticks as f32 * world.dt * TIME_SCALE;
            let most = scent.abs() / MAXIMUM * 100.0 / (HOURS_OF_SCENT * HOUR) * breathed;
            assert_eq!(change.signum() * f32::from(change != 0.0), scent.signum() * f32::from(scent != 0.0), "scent {scent}: {change}");
            assert!(change.abs() <= most + 1e-4, "scent {scent}: {change} over {breathed} world seconds, at most {most}");
            moved.push(change.abs());
        }
        assert!(moved[2] > 10.0 * moved[1] && moved[4] > 10.0 * moved[3], "stronger moves it more: {moved:?}");
        assert!(moved[4] < 0.2, "four looks at the worst stench are a sliver, not four points: {moved:?}");
    }
}
