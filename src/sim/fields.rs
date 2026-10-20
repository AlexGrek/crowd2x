//! [`Fields`]: the wheat the farmers grow — every crop in the world, whose
//! farm it is sown for, how old it is and how well it has been kept.
//!
//! **A crop is sown in a plot of good soil** (`Map::can_sow`: meadow, out in
//! the wasteland, both coordinates even) and stands in the way there, like a
//! heap of scrap, until it is harvested: the map keeps *that* it is there
//! (`Map::plant`), because passability is the map's, and this keeps the rest.
//!
//! **It grows on its own**: [`Stage`] is worked out from how long ago it was
//! sown, a stage every [`STAGE_DAYS`] days, ripe after [`GROWING_DAYS`] — a
//! seedling, young, in ear, ripe. Nothing about growing is stored, so nothing
//! is stepped every tick. **Tending it is what makes it good**: once a day
//! (not the day it was sown, and not once it is ripe) its farmer can tend it,
//! for [`TEND_QUALITY`] more quality, up to [`BEST`]; a ripe crop yields more
//! wheat the better it was kept ([`Crop::harvest`]). Sown on a Monday and
//! tended every day after but Sunday, a crop is ripe the next Sunday at its
//! best, and harvested and sown again on the Monday.
//!
//! **A crop is the farm's, not the farmer's**: it is sown for the farmer's
//! workplace, its own grain bin ([`Crop::farm`]), so a farmer let go or dead
//! leaves a field the next one hired to that bin takes on.
//!
//! Written only by the world step (`effect::apply`: `Effect::Sow`, `Tend`,
//! `Harvest`), read by every brain through `Think::fields`, and saved.

use crate::map::{is_plot, Map, Point};
use crate::save::{invalid, pb, Invalid};

use super::clock::{Clock, DAY};

/// How many crops a farmer keeps sown at once: its field.
pub const PLOTS: usize = 10;

/// How many days a crop takes from being sown to being ripe.
pub const GROWING_DAYS: u32 = 6;

/// How many days each stage before ripe lasts: three of them, then ripe.
pub const STAGE_DAYS: u32 = GROWING_DAYS / 3;

/// What a day's tending adds to a crop's quality, in percent.
pub const TEND_QUALITY: u8 = 20;

/// The best a crop can be kept: five days' tending, a whole week's but the
/// day it was sown and the Sunday.
pub const BEST: u8 = 100;

/// Quality worth one more sheaf of wheat at the harvest, on top of the one
/// every crop yields.
pub const QUALITY_PER_SHEAF: u8 = 40;

/// The most wheat one crop yields: kept at its [`BEST`].
pub const MOST_YIELDED: u8 = 1 + BEST / QUALITY_PER_SHEAF;

/// How far a crop has grown.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Stage {
    Seedling,
    Young,
    /// In ear: the heads of grain are out, still green.
    Heading,
    /// Gold, and ready to be harvested.
    Ripe,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Seedling, Stage::Young, Stage::Heading, Stage::Ripe];

    pub const fn name(self) -> &'static str {
        match self {
            Stage::Seedling => "seedling",
            Stage::Young => "young",
            Stage::Heading => "in ear",
            Stage::Ripe => "ripe",
        }
    }

    /// One to four, for "stage 2 of 4"; also which picture draws it.
    pub const fn number(self) -> u8 {
        self as u8 + 1
    }
}

/// One crop of wheat.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Crop {
    pub cell: Point,
    /// The farm it was sown for: the grain bin that is its farmer's workplace
    /// (`sim::job`), which whoever has that job keeps.
    pub farm: Point,
    /// When it was sown, in **world** seconds since the world began
    /// ([`Clock::elapsed`]).
    pub sown: f64,
    /// How well it has been kept, in percent: [`TEND_QUALITY`] for every day
    /// it was tended.
    pub quality: u8,
    /// The day it was last tended, or sown: a crop is tended at most once a
    /// day ([`Clock::day`]).
    pub tended: u32,
}

impl Crop {
    /// How far it has grown by `now`.
    pub fn stage(&self, now: Clock) -> Stage {
        let age = (now.elapsed() - self.sown).max(0.0);
        let stages = (age / (STAGE_DAYS as f64 * DAY as f64)) as usize;
        Stage::ALL[stages.min(Stage::ALL.len() - 1)]
    }

    pub fn is_ripe(&self, now: Clock) -> bool {
        self.stage(now) == Stage::Ripe
    }

    /// World seconds until it is ripe; zero once it is.
    pub fn ripe_in(&self, now: Clock) -> f64 {
        (self.sown + GROWING_DAYS as f64 * DAY as f64 - now.elapsed()).max(0.0)
    }

    /// Whether it wants tending today: growing still, and not yet tended (or
    /// sown) today.
    pub fn needs_tending(&self, now: Clock) -> bool {
        !self.is_ripe(now) && self.tended < now.day()
    }

    /// How many sheaves of wheat it yields, harvested ripe: one, and one more
    /// for every [`QUALITY_PER_SHEAF`] of its quality — one to
    /// [`MOST_YIELDED`].
    pub fn harvest(&self) -> u8 {
        1 + self.quality / QUALITY_PER_SHEAF
    }

    fn save(&self) -> pb::Crop {
        let Crop { cell, farm, sown, quality, tended } = *self;
        pb::Crop { cell: Some(cell.into()), farm: Some(farm.into()), sown, quality: quality as u32, tended }
    }
}

/// Every crop in the world, by cell.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Fields {
    /// Sorted by cell, so a cell is a binary search and the order is the same
    /// on every run.
    crops: Vec<Crop>,
}

impl Fields {
    pub fn new() -> Fields {
        Fields::default()
    }

    fn find(&self, cell: Point) -> Result<usize, usize> {
        self.crops.binary_search_by_key(&cell, |crop| crop.cell)
    }

    /// The crop in `cell`.
    pub fn at(&self, cell: Point) -> Option<&Crop> {
        self.find(cell).ok().map(|index| &self.crops[index])
    }

    /// Every crop, by cell.
    pub fn iter(&self) -> impl Iterator<Item = &Crop> + '_ {
        self.crops.iter()
    }

    /// The crops sown for the farm whose grain bin is at `farm`: a farmer's
    /// field. A walk of every crop — a district grows a few dozen.
    pub fn of(&self, farm: Point) -> impl Iterator<Item = &Crop> + '_ {
        self.crops.iter().filter(move |crop| crop.farm == farm)
    }

    pub fn len(&self) -> usize {
        self.crops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.crops.is_empty()
    }

    /// Sow a crop in `cell` for the farm at `farm`, at `now`: planted on the
    /// map, so it is in the way from now on. Refused — `false`, nothing
    /// changed — where the map will not have it ([`Map::can_sow`]) or the
    /// farm's field is already [`PLOTS`] crops.
    pub fn sow(&mut self, map: &mut Map, cell: Point, farm: Point, now: Clock) -> bool {
        let Err(at) = self.find(cell) else {
            return false;
        };
        if self.of(farm).count() >= PLOTS || !map.plant(cell) {
            return false;
        }
        self.crops.insert(at, Crop { cell, farm, sown: now.elapsed(), quality: 0, tended: now.day() });
        true
    }

    /// Tend the crop in `cell` today: its quality, raised — or `None`, and
    /// nothing done, where there is no crop or it does not want tending
    /// ([`Crop::needs_tending`]).
    pub fn tend(&mut self, cell: Point, now: Clock) -> Option<u8> {
        let index = self.find(cell).ok()?;
        let crop = &mut self.crops[index];
        if !crop.needs_tending(now) {
            return None;
        }
        crop.quality = crop.quality.saturating_add(TEND_QUALITY).min(BEST);
        crop.tended = now.day();
        Some(crop.quality)
    }

    /// Harvest the crop in `cell`: gone from the fields and the map, and
    /// handed back to say how much it yields ([`Crop::harvest`]). `None`, and
    /// nothing done, where there is no crop or it is not ripe.
    pub fn harvest(&mut self, map: &mut Map, cell: Point, now: Clock) -> Option<Crop> {
        let index = self.find(cell).ok()?;
        if !self.crops[index].is_ripe(now) {
            return None;
        }
        let crop = self.crops.remove(index);
        map.unplant(cell);
        Some(crop)
    }

    // --- saving ---

    pub(crate) fn save(&self) -> Vec<pb::Crop> {
        self.crops.iter().map(Crop::save).collect()
    }

    /// The fields a save had, planted on `map` again — which refuses a crop
    /// anywhere a crop could not have been sown.
    pub(crate) fn restore(saved: &[pb::Crop], map: &mut Map) -> Result<Fields, Invalid> {
        let mut crops = Vec::with_capacity(saved.len());
        for crop in saved {
            let (Some(cell), Some(farm)) = (crop.cell, crop.farm) else {
                return invalid("a crop with no cell or no farm");
            };
            let (cell, farm) = (Point::from(cell), Point::from(farm));
            if !(crop.sown.is_finite() && crop.sown >= 0.0) || crop.quality > BEST as u32 {
                return invalid(format!("a crop at {}, {} sown at {} of quality {}", cell.x, cell.y, crop.sown, crop.quality));
            }
            if !is_plot(cell) || !map.plant(cell) {
                return invalid(format!("a crop at {}, {}, where nothing can be sown", cell.x, cell.y));
            }
            crops.push(Crop { cell, farm, sown: crop.sown, quality: crop.quality as u8, tended: crop.tended });
        }
        crops.sort_by_key(|crop| crop.cell);
        Ok(Fields { crops })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, MEADOW};
    use crate::sim::clock::{HOUR, OPENING_TIME, TIME_SCALE};

    /// `hours` o'clock on day `day` — day 1 is a Monday.
    fn at(day: u32, hours: f32) -> Clock {
        let world = (day - 1) as f64 * DAY as f64 + hours as f64 * HOUR as f64 - OPENING_TIME;
        Clock::after_watching(world / TIME_SCALE as f64)
    }

    const FARM: Point = Point::new(0, 1);

    fn field() -> (Map, Fields) {
        (Map::new(Size::new(12, 12), MEADOW), Fields::new())
    }

    #[test]
    fn a_crop_grows_a_stage_every_two_days_and_is_ripe_after_six() {
        let (mut map, mut fields) = field();
        let plot = Point::new(4, 4);
        assert!(fields.sow(&mut map, plot, FARM, at(1, 10.0)));
        let crop = *fields.at(plot).unwrap();
        let stages: Vec<Stage> = [(1, 10.0), (3, 9.9), (3, 10.0), (5, 10.0), (7, 9.9), (7, 10.0), (30, 0.0)]
            .into_iter()
            .map(|(day, hours)| crop.stage(at(day, hours)))
            .collect();
        use Stage::*;
        assert_eq!(stages, [Seedling, Seedling, Young, Heading, Heading, Ripe, Ripe]);
        assert_eq!(crop.ripe_in(at(1, 10.0)), 6.0 * DAY as f64);
        assert_eq!(crop.ripe_in(at(8, 0.0)), 0.0);
        assert_eq!(Ripe.number(), 4);
    }

    /// Once a day, never the day it went in and never once ripe: tended
    /// every day but the Sunday, a crop sown on a Monday is at its best when
    /// it is ripe, and yields the most.
    #[test]
    fn a_crop_is_tended_once_a_day_and_yields_more_the_better_it_was_kept() {
        let (mut map, mut fields) = field();
        let plot = Point::new(2, 6);
        assert!(fields.sow(&mut map, plot, FARM, at(1, 10.0)));
        assert_eq!(fields.tend(plot, at(1, 15.0)), None, "not the day it was sown");
        for day in 2..=6 {
            assert_eq!(fields.tend(plot, at(day, 11.0)), Some(TEND_QUALITY * (day as u8 - 1)), "day {day}");
            assert_eq!(fields.tend(plot, at(day, 12.0)), None, "day {day}: once a day");
        }
        assert_eq!(fields.at(plot).unwrap().quality, BEST);
        assert_eq!(fields.tend(plot, at(7, 11.0)), None, "ripe: nothing to tend");
        assert_eq!(fields.at(plot).unwrap().harvest(), MOST_YIELDED);
        let neglected = Crop { quality: 0, ..*fields.at(plot).unwrap() };
        assert_eq!(neglected.harvest(), 1);
        assert_eq!(Crop { quality: 60, ..neglected }.harvest(), 2);
    }

    #[test]
    fn only_a_ripe_crop_is_harvested_and_harvesting_it_opens_the_plot() {
        let (mut map, mut fields) = field();
        let plot = Point::new(6, 6);
        assert!(fields.sow(&mut map, plot, FARM, at(1, 10.0)));
        assert!(!map.is_passable(plot), "a crop is in the way");
        assert_eq!(fields.harvest(&mut map, plot, at(5, 10.0)), None, "still in ear");
        let crop = fields.harvest(&mut map, plot, at(7, 10.0)).expect("ripe");
        assert_eq!(crop.cell, plot);
        assert!(fields.at(plot).is_none() && map.is_passable(plot));
        assert_eq!(fields.harvest(&mut map, plot, at(7, 10.0)), None, "nothing left");
    }

    #[test]
    fn a_farm_sows_up_to_its_field_and_only_in_plots_of_good_soil() {
        let (mut map, mut fields) = field();
        assert!(!fields.sow(&mut map, Point::new(3, 4), FARM, at(1, 10.0)), "not a plot");
        assert!(!fields.sow(&mut map, Point::new(0, 4), FARM, at(1, 10.0)), "the map's edge, nothing past it");
        // Every plot with the ground open all round it.
        let plots: Vec<Point> = (1..6).flat_map(|y| (1..6).map(move |x| Point::new(2 * x, 2 * y))).collect();
        let sown = plots.iter().filter(|&&plot| fields.sow(&mut map, plot, FARM, at(1, 10.0))).count();
        assert_eq!(sown, PLOTS);
        assert_eq!(fields.of(FARM).count(), PLOTS);
        assert!(!fields.sow(&mut map, plots[0], Point::new(9, 9), at(1, 10.0)), "already sown");
        assert!(fields.sow(&mut map, plots[PLOTS], Point::new(9, 9), at(1, 10.0)), "another farm's field");
        assert!(fields.iter().zip(fields.iter().skip(1)).all(|(a, b)| a.cell < b.cell), "by cell");
    }

    #[test]
    fn the_fields_come_back_from_a_save_and_are_planted_again() {
        let (mut map, mut fields) = field();
        for plot in [Point::new(2, 2), Point::new(8, 4)] {
            assert!(fields.sow(&mut map, plot, FARM, at(1, 10.0)));
        }
        let _ = fields.tend(Point::new(8, 4), at(2, 10.0));
        let saved = fields.save();
        let mut fresh = Map::new(Size::new(12, 12), MEADOW);
        let restored = Fields::restore(&saved, &mut fresh).unwrap();
        assert_eq!(restored.crops, fields.crops);
        assert!(!fresh.is_passable(Point::new(2, 2)) && !fresh.is_passable(Point::new(8, 4)));

        let mut again = Map::new(Size::new(12, 12), MEADOW);
        let odd = pb::Crop { cell: Some(Point::new(3, 3).into()), ..saved[0] };
        assert!(Fields::restore(&[odd], &mut again).is_err(), "not a plot");
        let twice = [saved[0], saved[0]];
        assert!(Fields::restore(&twice, &mut Map::new(Size::new(12, 12), MEADOW)).is_err(), "one crop to a plot");
        let too_good = pb::Crop { quality: 101, ..saved[0] };
        assert!(Fields::restore(&[too_good], &mut Map::new(Size::new(12, 12), MEADOW)).is_err());
        let floor = &mut Map::new(Size::new(12, 12), crate::map::FLOOR);
        assert!(Fields::restore(&saved, floor).is_err(), "no soil");
    }
}
