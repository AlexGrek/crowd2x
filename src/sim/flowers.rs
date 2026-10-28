//! Walkable flower beds that grow, bloom, wither and leave room for replanting.
//! Stages follow the saved planting time; cleanup sleeps until a bed expires.

use super::clock::{Clock, DAY};
use crate::map::{Map, Point};
use crate::save::{Invalid, invalid, pb};

pub const VARIETIES: usize = 6;
/// One world day growing, three in bloom, then one withered before decaying.
pub const GROW_SECONDS: f64 = DAY as f64;
pub const WITHER_AGE: f64 = 4.0 * DAY as f64;
pub const LIFESPAN: f64 = 5.0 * DAY as f64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Seedling,
    Blooming,
    Withered,
}

pub const NAMES: [&str; VARIETIES] = [
    "red poppies",
    "yellow daisies",
    "blue cornflowers",
    "pink tulips",
    "white daisies",
    "purple bellflowers",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flower {
    pub cell: Point,
    pub variety: u8,
    pub sown: f64,
}

impl Flower {
    pub fn stage(&self, now: Clock) -> Stage {
        let age = now.elapsed() - self.sown;
        if age < GROW_SECONDS {
            Stage::Seedling
        } else if age < WITHER_AGE {
            Stage::Blooming
        } else {
            Stage::Withered
        }
    }

    pub fn blooming(&self, now: Clock) -> bool {
        self.stage(now) == Stage::Blooming
    }
}

/// A spaced bed on outdoor grass inside the city. Flowers never close a route.
pub fn green_plot(map: &Map, cell: Point) -> bool {
    map.contains(cell)
        && cell.x.rem_euclid(2) == 0
        && cell.y.rem_euclid(2) == 0
        && !map.has_ceiling(cell)
        && map.is_passable(cell)
        && map.terrain(cell).is_some_and(|tile| tile.name() == "grass")
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Flowers {
    beds: Vec<Flower>,
    /// Derived from planting times, never saved as a second clock.
    next_decay: Option<f64>,
}

impl Flowers {
    pub fn at(&self, cell: Point) -> Option<&Flower> {
        self.beds
            .binary_search_by_key(&cell, |bed| bed.cell)
            .ok()
            .map(|i| &self.beds[i])
    }

    pub fn iter(&self) -> impl Iterator<Item = &Flower> {
        self.beds.iter()
    }

    pub fn sow(&mut self, map: &Map, cell: Point, variety: u8, now: Clock) -> bool {
        let Err(index) = self.beds.binary_search_by_key(&cell, |bed| bed.cell) else {
            return false;
        };
        if !green_plot(map, cell) || variety as usize >= VARIETIES {
            return false;
        }
        self.beds.insert(
            index,
            Flower {
                cell,
                variety,
                sown: now.elapsed(),
            },
        );
        let decay = now.elapsed() + LIFESPAN;
        self.next_decay = Some(self.next_decay.map_or(decay, |next| next.min(decay)));
        true
    }

    /// Remove dead beds before brains choose plots, without scanning on other ticks.
    pub(crate) fn advance(&mut self, now: Clock) {
        if !self.next_decay.is_some_and(|next| now.elapsed() >= next) {
            return;
        }
        self.next_decay = None;
        self.beds.retain(|bed| {
            let decay = bed.sown + LIFESPAN;
            if now.elapsed() >= decay {
                return false;
            }
            self.next_decay = Some(self.next_decay.map_or(decay, |next| next.min(decay)));
            true
        });
    }

    pub(crate) fn save(&self) -> Vec<pb::Flower> {
        self.beds
            .iter()
            .map(|bed| pb::Flower {
                cell: Some(bed.cell.into()),
                variety: NAMES[bed.variety as usize].to_string(),
                sown: bed.sown,
            })
            .collect()
    }

    pub(crate) fn restore(saved: &[pb::Flower], map: &Map, now: Clock) -> Result<Self, Invalid> {
        let mut flowers = Self::default();
        for bed in saved {
            let cell = pb::cell(bed.cell, "flower bed")?;
            let variety = NAMES
                .iter()
                .position(|&name| name == bed.variety)
                .ok_or_else(|| Invalid::new("unknown flower variety"))?
                as u8;
            if !bed.sown.is_finite()
                || bed.sown < 0.0
                || bed.sown > now.elapsed()
                || !flowers.sow(map, cell, variety, now)
            {
                return invalid("invalid or duplicate flower bed");
            }
            let index = flowers
                .beds
                .binary_search_by_key(&cell, |bed| bed.cell)
                .unwrap();
            flowers.beds[index].sown = bed.sown;
        }
        flowers.next_decay = flowers
            .beds
            .iter()
            .map(|bed| bed.sown + LIFESPAN)
            .reduce(f64::min);
        Ok(flowers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, TERRAIN};

    fn at(age: f64) -> Clock {
        Clock::after_watching(age / super::super::clock::TIME_SCALE as f64)
    }

    #[test]
    fn flowers_bloom_then_wither_and_decay_at_their_world_time_boundaries() {
        let map = park();
        let cell = Point::new(4, 4);
        let mut flowers = Flowers::default();
        assert!(flowers.sow(&map, cell, 0, at(0.0)));
        for (age, stage) in [
            (0.0, Stage::Seedling),
            (GROW_SECONDS - 1.0, Stage::Seedling),
            (GROW_SECONDS, Stage::Blooming),
            (WITHER_AGE - 1.0, Stage::Blooming),
            (WITHER_AGE, Stage::Withered),
            (LIFESPAN - 1.0, Stage::Withered),
        ] {
            flowers.advance(at(age));
            let bed = flowers.at(cell).expect("still alive");
            assert_eq!(bed.stage(at(age)), stage);
            assert_eq!(bed.blooming(at(age)), stage == Stage::Blooming);
        }
        flowers.advance(at(LIFESPAN));
        assert!(flowers.at(cell).is_none());
        assert!(map.is_passable(cell));
        assert!(flowers.sow(&map, cell, 1, at(LIFESPAN)));
        assert_eq!(
            flowers.at(cell).unwrap().stage(at(LIFESPAN)),
            Stage::Seedling
        );
    }

    #[test]
    fn staggered_beds_keep_their_ages_across_saves_and_large_time_steps() {
        let map = park();
        let mut flowers = Flowers::default();
        // Plant in reverse cell order: the next expiry isn't the first cell.
        let older = Point::new(6, 4);
        let younger = Point::new(2, 2);
        assert!(flowers.sow(&map, older, 1, at(0.0)));
        assert!(flowers.sow(&map, younger, 2, at(DAY as f64)));
        flowers.advance(at(WITHER_AGE));
        let mut loaded = Flowers::restore(&flowers.save(), &map, at(WITHER_AGE)).unwrap();
        assert_eq!(loaded, flowers);
        assert_eq!(
            loaded.at(older).unwrap().stage(at(WITHER_AGE)),
            Stage::Withered
        );
        assert!(loaded.at(younger).unwrap().blooming(at(WITHER_AGE)));
        for age in [LIFESPAN, LIFESPAN + DAY as f64 - 1.0, 20.0 * DAY as f64] {
            loaded.advance(at(age));
            flowers.advance(at(age));
            assert_eq!(loaded, flowers);
            assert!(loaded.at(older).is_none());
            assert_eq!(loaded.at(younger).is_some(), age < LIFESPAN + DAY as f64);
        }
        assert_eq!(loaded, Flowers::default());
    }

    #[test]
    fn expired_saved_beds_are_validated_before_they_are_removed() {
        let map = park();
        let mut flowers = Flowers::default();
        assert!(flowers.sow(&map, Point::new(4, 4), 0, at(0.0)));
        let saved = flowers.save();
        let mut loaded = Flowers::restore(&saved, &map, at(20.0 * DAY as f64)).unwrap();
        loaded.advance(at(20.0 * DAY as f64));
        assert_eq!(loaded, Flowers::default());
        let duplicates = vec![saved[0].clone(); 2];
        assert!(Flowers::restore(&duplicates, &map, at(LIFESPAN)).is_err());
        for sown in [f64::NAN, f64::INFINITY, -1.0, LIFESPAN + 1.0] {
            let mut invalid = saved[0].clone();
            invalid.sown = sown;
            assert!(Flowers::restore(&[invalid], &map, at(LIFESPAN)).is_err());
        }
    }

    #[test]
    fn the_real_simulation_decays_flowers_even_without_any_decorators() {
        let mut state = super::super::GameState::new(park(), 1);
        let cell = Point::new(4, 4);
        assert!(
            state
                .facilities
                .flowers
                .sow(&state.map, cell, 0, state.clock())
        );
        let dt = (LIFESPAN / super::super::clock::TIME_SCALE as f64) as f32;
        let mut old_save = state.save_world();
        old_save.elapsed = dt as f64;
        let old_loaded =
            super::super::GameState::restore(state.map.clone(), &old_save, Vec::new()).unwrap();
        assert!(
            old_loaded.flowers().at(cell).is_none(),
            "old eternal beds decay on load"
        );
        super::super::process_pass(&mut state, dt - 1.0);
        assert_eq!(
            state.flowers().at(cell).unwrap().stage(state.clock()),
            Stage::Withered
        );
        let mut loaded =
            super::super::GameState::restore(state.map.clone(), &state.save_world(), Vec::new())
                .unwrap();
        assert_eq!(loaded.flowers(), state.flowers());
        super::super::process_pass(&mut state, 1.0);
        super::super::process_pass(&mut loaded, 1.0);
        assert!(state.flowers().at(cell).is_none());
        assert_eq!(loaded.flowers(), state.flowers());
    }

    pub(crate) fn park() -> Map {
        let grass = TERRAIN
            .iter()
            .position(|tile| tile.name == "grass")
            .unwrap();
        Map::new(Size::new(14, 10), crate::map::TerrainId(grass as u16))
    }

    #[test]
    fn flowers_grow_in_city_grass_remain_walkable_and_survive_saves() {
        let map = park();
        let mut flowers = Flowers::default();
        let cell = Point::new(4, 4);
        let now = Clock::after_watching(0.0);
        assert!(flowers.sow(&map, cell, 3, now));
        assert!(!flowers.sow(&map, cell, 1, now));
        assert!(!flowers.sow(&map, Point::new(3, 4), 0, now));
        assert!(!flowers.sow(&Map::new(map.size(), crate::map::FLOOR), cell, 0, now));
        assert!(map.is_passable(cell));
        assert!(!flowers.at(cell).unwrap().blooming(now));
        let tomorrow = Clock::after_watching((DAY / super::super::clock::TIME_SCALE) as f64);
        assert!(flowers.at(cell).unwrap().blooming(tomorrow));
        assert_eq!(
            Flowers::restore(&flowers.save(), &map, tomorrow).unwrap(),
            flowers
        );
        let bad = pb::Flower {
            variety: "unknown flower".into(),
            ..flowers.save()[0].clone()
        };
        assert!(Flowers::restore(&[bad], &map, tomorrow).is_err());
        let twice = vec![flowers.save()[0].clone(); 2];
        assert!(Flowers::restore(&twice, &map, tomorrow).is_err());
    }
}
