//! [`Playzones`]: which arcade cabinets are working right now.
//!
//! A playzone is a **central computer** (`"playzone computer"`) and the arcade
//! cabinets (`"arcade cabinet"`) it runs. A cabinet works only while the
//! playzone is run: an administrator ([`JobKind::PlayzoneAdministrator`]) *on
//! duty* — in the hours of their shift — and *at the computer*, beside it.
//! With nobody there, or off shift, or away from it, every cabinet is dark and
//! nobody goes to one; a game cut short by the administrator leaving is no fun
//! at all, as a go on a computer is when its power goes
//! ([`UseArcade`](super::brain::tasks::UseArcade)).
//!
//! **Which computer runs which cabinet** is fixed by the map: the nearest
//! ([`Playzones::from_map`]), so a map may have several playzones, and a
//! cabinet with no computer at all never works.
//!
//! **Who is running a computer** is the administrator's to say, every tick it
//! is, with [`Effect::Operate`] — `Brain::react` asks it of the world
//! whenever [`runs`] holds — and the world step grants it
//! ([`Playzones::run`]) after clearing last tick's answer
//! ([`Playzones::begin_tick`]). So what a brain reads is the state as of the
//! end of the previous tick, like a fridge's door, and an administrator who
//! walks away, is let go, dies or whose shift ends is simply not saying so the
//! next tick: nothing to forget to undo. Power is not here — a cabinet or
//! computer with none is not a feature ([`Features`](super::feature::Features)).
//!
//! Saved ([`Playzones::save`]) so a loaded world carries on exactly: which
//! computers were being run when the tick ended. Everything else is rebuilt
//! from the map.

use crate::map::{Map, ObjectLayer, Point};
use crate::save::{invalid, pb, Invalid};

use super::entity::Think;
use super::feature::FeatureKind;
use super::job::Job;

/// Whether somebody with `job`, standing in `at`, is running the playzone they
/// work at right now: their kind of job is run from a playzone computer, their
/// shift's hours are on (the commute before them is not), they are beside the
/// computer, and it works. What `Brain::react` asks every tick of a worker.
pub fn runs(job: &Job, at: Point, think: &Think<'_>) -> bool {
    job.kind.workplace() == FeatureKind::PlayzoneComputer
        && job.shift.is_working_hours(think.clock)
        && at.manhattan_distance(job.workplace) <= 1
        && think.features.has(FeatureKind::PlayzoneComputer, job.workplace)
}

/// Every cabinet and the computer that runs it, and which computers are being
/// run.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Playzones {
    /// `(cabinet, its computer)`, sorted by cabinet. A cabinet on a map with
    /// no computer is not here, and so never works.
    cabinets: Vec<(Point, Point)>,
    /// Every playzone computer on the map, sorted: the only cells that can be
    /// run.
    computers: Vec<Point>,
    /// The computers an administrator was running as the last tick ended,
    /// sorted.
    run: Vec<Point>,
}

impl Playzones {
    /// The playzones of `map`, none of them run: each cabinet goes with the
    /// nearest computer (ties to the lower cell), power or no power — whether
    /// either works is the features'.
    pub fn from_map(map: &Map) -> Playzones {
        let (cabinet, computer) = (FeatureKind::Arcade.prop(), FeatureKind::PlayzoneComputer.prop());
        let mut computers: Vec<Point> = Vec::new();
        let mut cabinets: Vec<Point> = Vec::new();
        for object in map.objects(ObjectLayer::Props) {
            match object.kind.as_str() {
                name if name == computer => computers.push(object.cell()),
                name if name == cabinet => cabinets.push(object.cell()),
                _ => {}
            }
        }
        computers.sort_unstable();
        computers.dedup();
        cabinets.sort_unstable();
        cabinets.dedup();
        let cabinets = cabinets
            .into_iter()
            .filter_map(|cabinet| {
                let nearest = computers.iter().copied().min_by_key(|&each| (cabinet.manhattan_distance(each), each))?;
                Some((cabinet, nearest))
            })
            .collect();
        Playzones { cabinets, computers, run: Vec::new() }
    }

    /// Whether the cabinet in `cabinet` is working: its computer is being run.
    pub fn runs(&self, cabinet: Point) -> bool {
        self.cabinets
            .binary_search_by_key(&cabinet, |&(each, _)| each)
            .is_ok_and(|index| self.is_run(self.cabinets[index].1))
    }

    /// Whether an administrator was running the computer in `computer` as the
    /// last tick ended.
    pub fn is_run(&self, computer: Point) -> bool {
        self.run.binary_search(&computer).is_ok()
    }

    /// Every computer being run, sorted.
    pub fn running(&self) -> &[Point] {
        &self.run
    }

    /// How many cabinets work now.
    pub fn working(&self) -> usize {
        self.cabinets.iter().filter(|&&(_, computer)| self.is_run(computer)).count()
    }

    /// Every playzone computer on the map.
    pub fn computers(&self) -> &[Point] {
        &self.computers
    }

    /// Forget who was running what: the first thing the world step does, so
    /// the administrators still at their computers say so again this tick
    /// ([`Effect::Operate`](super::Effect::Operate)) and nobody else is
    /// counted.
    pub fn begin_tick(&mut self) {
        self.run.clear();
    }

    /// An administrator is running the computer in `computer`: `false`, and
    /// nothing changes, for a cell that is none.
    pub fn run(&mut self, computer: Point) -> bool {
        if self.computers.binary_search(&computer).is_err() {
            return false;
        }
        if let Err(index) = self.run.binary_search(&computer) {
            self.run.insert(index, computer);
        }
        true
    }

    // --- saving ---

    pub(crate) fn save(&self) -> Vec<pb::Cell> {
        let Playzones { cabinets: _, computers: _, run } = self;
        run.iter().map(|&cell| cell.into()).collect()
    }

    /// The computers being run, as saved: each has to be a playzone computer
    /// on this map.
    pub(crate) fn restore(&mut self, saved: &[pb::Cell]) -> Result<(), Invalid> {
        self.run.clear();
        for &cell in saved {
            let cell = Point::from(cell);
            if !self.run(cell) {
                return invalid(format!("an administrator runs the playzone at {}, {}, where there is no computer", cell.x, cell.y));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR};
    use crate::sim::testing::prop_at;

    fn map() -> Map {
        let mut map = Map::new(Size::new(24, 8), FLOOR);
        prop_at(&mut map, "playzone computer", Point::new(3, 3));
        prop_at(&mut map, "playzone computer", Point::new(18, 3));
        for x in [1, 5, 16, 21] {
            prop_at(&mut map, "arcade cabinet", Point::new(x, 1));
        }
        map
    }

    #[test]
    fn a_cabinet_goes_with_the_nearest_computer_and_works_only_while_it_is_run() {
        let mut zones = Playzones::from_map(&map());
        assert_eq!(zones.computers(), [Point::new(3, 3), Point::new(18, 3)]);
        assert_eq!(zones.working(), 0, "nobody there: all dark");
        assert!(!zones.runs(Point::new(1, 1)));

        assert!(zones.run(Point::new(3, 3)));
        assert!(zones.runs(Point::new(1, 1)) && zones.runs(Point::new(5, 1)), "the west playzone's");
        assert!(!zones.runs(Point::new(16, 1)) && !zones.runs(Point::new(21, 1)), "the east one is still dark");
        assert_eq!(zones.working(), 2);
        assert!(!zones.runs(Point::new(9, 9)), "no cabinet there at all");

        zones.begin_tick();
        assert_eq!(zones.working(), 0, "everybody has to say so again each tick");
    }

    #[test]
    fn only_a_playzone_computer_can_be_run_and_the_same_one_twice_is_once() {
        let mut zones = Playzones::from_map(&map());
        assert!(!zones.run(Point::new(4, 4)));
        assert!(zones.run(Point::new(18, 3)) && zones.run(Point::new(18, 3)));
        assert_eq!(zones.running(), [Point::new(18, 3)]);
    }

    #[test]
    fn a_cabinet_with_no_computer_never_works() {
        let mut map = Map::new(Size::new(8, 8), FLOOR);
        prop_at(&mut map, "arcade cabinet", Point::new(2, 2));
        let mut zones = Playzones::from_map(&map);
        assert!(zones.computers().is_empty());
        assert!(!zones.run(Point::new(2, 2)));
        assert!(!zones.runs(Point::new(2, 2)));
    }

    #[test]
    fn what_is_run_is_saved_and_a_cell_that_is_no_computer_is_refused() {
        let mut zones = Playzones::from_map(&map());
        zones.run(Point::new(18, 3));
        let saved = zones.save();
        let mut loaded = Playzones::from_map(&map());
        loaded.restore(&saved).unwrap();
        assert_eq!(loaded, zones);

        let bad = [pb::Cell::from(Point::new(0, 0))];
        assert!(loaded.restore(&bad).is_err());
    }
}
