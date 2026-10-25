//! Permanent, walkable flower beds in the city's public green zones.
//! Growth is derived from the clock, with no work each tick.

use super::clock::{Clock, DAY};
use crate::map::{Map, Point};
use crate::save::{Invalid, invalid, pb};

pub const VARIETIES: usize = 6;
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
    pub fn blooming(&self, now: Clock) -> bool {
        now.elapsed() - self.sown >= DAY as f64
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
        true
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
        Ok(flowers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, TERRAIN};

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
