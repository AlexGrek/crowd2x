//! Religion: how religious each person is, the church's clerics, and prayer.
//!
//! * **Religiosity** — 0, not at all, to 100, devout — is who somebody is,
//!   rolled once from its uid ([`religiosity`]) and kept on its
//!   [`Body`](super::entity::Body), like obedience. It never changes by
//!   itself.
//! * **The clerics** are the church's: [`JobKind::Cleric`](super::job::JobKind::Cleric),
//!   one to each of its altars, and **the most religious of the free** —
//!   never offered to an arrival by distance as every other job is, but
//!   ordained, the most religious living jobless person first
//!   (`GameState::ordain`), once everybody has moved in and on every hour
//!   after that a post stands empty. Each leads prayers at its own altar
//!   through its shift (`WorkGoal`, the `"lead prayers"` stretch).
//! * **Prayer**: somebody religious enough ([`MIN_FAITH`]) wants to pray now
//!   and then while it has nothing better to do — the more religious, the
//!   oftener ([`prayer_gap`]) — walks to the nearest pew and prays beside it
//!   ([`Task::pray`](super::brain::task::Task::pray), [`PRAYER_SECONDS`]).
//!   A prayer mends the mind by how devout the one praying is
//!   (`Event::Prayed`, the `Mind` process), **twice as much at a service**:
//!   while a cleric is leading prayers at an altar of the church
//!   ([`Services`]).
//!
//! [`Services`] is the playzone's pattern ([`super::playzone`]): a cleric on
//! duty and at its altar says so every tick ([`leads`], `Effect::Lead`), the
//! world step forgets last tick's answer first, and a cleric who walks away
//! simply stops saying so.

use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

use crate::map::{Map, ObjectLayer, Point};
use crate::save::{invalid, pb, Invalid};

use super::clock::{watched, HOUR, MINUTE};
use super::entity::Think;
use super::feature::FeatureKind;
use super::job::Job;
use super::uid::Uid;

/// How religious somebody is, 0 to 100: rolled evenly, from a stream of its
/// own uid's — so every other roll a world makes is the one it made before
/// there was religion, and a save from before rolls the same value again.
pub fn religiosity(uid: Uid) -> u8 {
    SmallRng::seed_from_u64(uid.raw() ^ 0x7265_6c69_6769_6f6e).random_range(0..=100)
}

/// Religiosity under this never prays: it does not believe enough to go.
pub const MIN_FAITH: u8 = 10;

/// How long after a prayer before the most devout (100) wants the next, in
/// world seconds: half a day. Somebody half as religious waits twice as long.
pub const PRAYER_GAP_AT_FULL: f64 = 12.0 * HOUR as f64;

/// How long after its last prayer somebody of `religiosity` wants to pray
/// again, world seconds: [`PRAYER_GAP_AT_FULL`] scaled by how far short of
/// devout it is — 12 hours at 100, a day at 50, five at [`MIN_FAITH`] — and
/// never, under it.
pub fn prayer_gap(religiosity: u8) -> f64 {
    if religiosity < MIN_FAITH {
        return f64::INFINITY;
    }
    PRAYER_GAP_AT_FULL * 100.0 / religiosity as f64
}

/// Somebody due a prayer rolls for it once a slot this long (world
/// seconds)...
pub const PRAYER_SLOT: f64 = 30.0 * 60.0;

/// ...and sets off in one slot in this many: two hours' wandering, on
/// average, after it is due — so a crowd does not set off for the church at
/// one moment.
pub const PRAYER_ONE_IN: u64 = 4;

/// Twenty world minutes beside a pew, in prayer.
pub const PRAYER_SECONDS: f32 = watched(20.0 * MINUTE);

/// How far from a pew (manhattan, cells) an altar being led makes its prayer
/// a service: the church, whichever way it was built.
pub const SERVICE_REACH: i32 = 12;

/// Whether somebody with `job`, standing in `at`, is leading prayers at its
/// altar right now: its job is worked at an altar, its shift's hours are on
/// (the commute before them is not), and it stands beside the altar. What
/// `Brain::react` asks every tick of a worker.
pub fn leads(job: &Job, at: Point, think: &Think<'_>) -> bool {
    job.kind.workplace() == FeatureKind::Altar
        && job.shift.is_working_hours(think.clock)
        && at.manhattan_distance(job.workplace) <= 1
        && think.features.has(FeatureKind::Altar, job.workplace)
}

/// Every altar on the map, and which a cleric was leading prayers at as the
/// last tick ended.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Services {
    /// Every altar, sorted: the only cells prayers can be led at.
    altars: Vec<Point>,
    /// The altars a cleric was at, on duty, as the last tick ended, sorted.
    led: Vec<Point>,
}

impl Services {
    /// The map's altars, none of them led.
    pub fn from_map(map: &Map) -> Services {
        let altar = FeatureKind::Altar.prop();
        let mut altars: Vec<Point> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|object| object.kind.as_str() == altar)
            .map(|object| object.cell())
            .collect();
        altars.sort_unstable();
        altars.dedup();
        Services { altars, led: Vec::new() }
    }

    /// Whether a prayer beside the pew in `pew` is at a service: a cleric is
    /// leading prayers at an altar within [`SERVICE_REACH`] of it. A handful
    /// of altars at most, so a walk of them.
    pub fn is_on(&self, pew: Point) -> bool {
        self.led.iter().any(|&altar| altar.manhattan_distance(pew) <= SERVICE_REACH)
    }

    /// Whether a cleric was leading prayers at the altar in `altar` as the
    /// last tick ended.
    pub fn is_led(&self, altar: Point) -> bool {
        self.led.binary_search(&altar).is_ok()
    }

    /// Every altar being led, sorted.
    pub fn led(&self) -> &[Point] {
        &self.led
    }

    /// Every altar on the map.
    pub fn altars(&self) -> &[Point] {
        &self.altars
    }

    /// Forget who was leading what: the first thing the world step does, so
    /// the clerics still at their altars say so again this tick.
    pub fn begin_tick(&mut self) {
        self.led.clear();
    }

    /// A cleric is leading prayers at `altar`: `false`, and nothing changes,
    /// for a cell that is no altar.
    pub fn lead(&mut self, altar: Point) -> bool {
        if self.altars.binary_search(&altar).is_err() {
            return false;
        }
        if let Err(index) = self.led.binary_search(&altar) {
            self.led.insert(index, altar);
        }
        true
    }

    // --- saving ---

    pub(crate) fn save(&self) -> Vec<pb::Cell> {
        let Services { altars: _, led } = self;
        led.iter().map(|&cell| cell.into()).collect()
    }

    /// The altars being led, as saved: each has to be an altar on this map.
    pub(crate) fn restore(&mut self, saved: &[pb::Cell]) -> Result<(), Invalid> {
        self.led.clear();
        for &cell in saved {
            let cell: Point = cell.into();
            if !self.lead(cell) {
                return invalid(format!("prayers led at {}, {}, where there is no altar", cell.x, cell.y));
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
    use crate::sim::uid::EntityType;

    /// Everybody from nought to a hundred, evenly, and the same again for the
    /// same person.
    #[test]
    fn religiosity_is_rolled_evenly_from_nought_to_a_hundred_and_is_the_same_for_the_same_person() {
        let rolls: Vec<u8> = (1..=2000).map(|n| religiosity(Uid::new(EntityType::Human, n))).collect();
        assert!(rolls.iter().all(|&r| r <= 100));
        assert!(rolls.contains(&0) && rolls.contains(&100), "both ends are rolled");
        let mean = rolls.iter().map(|&r| r as f64).sum::<f64>() / rolls.len() as f64;
        assert!((mean - 50.0).abs() < 3.0, "evenly: {mean}");
        let low = rolls.iter().filter(|&&r| r < MIN_FAITH).count();
        assert!((100..300).contains(&low), "about one in ten never prays: {low}");
        let one = Uid::new(EntityType::Human, 7);
        assert_eq!(religiosity(one), religiosity(one));
    }

    #[test]
    fn the_more_religious_pray_the_oftener_and_the_faithless_never() {
        assert_eq!(prayer_gap(100), PRAYER_GAP_AT_FULL);
        assert_eq!(prayer_gap(50), 2.0 * PRAYER_GAP_AT_FULL);
        assert!(prayer_gap(MIN_FAITH).is_finite());
        assert!(prayer_gap(MIN_FAITH - 1).is_infinite());
        assert!(prayer_gap(0).is_infinite());
    }

    /// Only an altar is led; a prayer is at a service near one being led and
    /// nowhere else; the next tick forgets it; and the save keeps it.
    #[test]
    fn a_service_is_on_near_an_altar_a_cleric_leads_and_only_while_it_does() {
        let mut map = Map::new(Size::new(40, 10), FLOOR);
        prop_at(&mut map, "altar", Point::new(2, 8));
        prop_at(&mut map, "altar", Point::new(3, 8));
        let mut services = Services::from_map(&map);
        assert_eq!(services.altars(), [Point::new(2, 8), Point::new(3, 8)]);
        let (near, far) = (Point::new(3, 2), Point::new(30, 2));
        assert!(!services.is_on(near), "nobody leads anything yet");
        assert!(!services.lead(Point::new(5, 5)), "no altar there");
        assert!(services.lead(Point::new(3, 8)));
        assert!(services.is_led(Point::new(3, 8)) && !services.is_led(Point::new(2, 8)));
        assert!(services.is_on(near));
        assert!(!services.is_on(far), "a pew across the map is not at this service");

        let saved = services.save();
        let mut again = Services::from_map(&map);
        again.restore(&saved).unwrap();
        assert_eq!(again, services);
        assert!(Services::from_map(&Map::new(Size::new(9, 9), FLOOR)).restore(&saved).is_err(), "no altar there");

        services.begin_tick();
        assert!(!services.is_on(near), "a cleric who stops saying so leads nothing");
    }
}
