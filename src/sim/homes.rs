//! Who sleeps where: [`Homes`], the beds that have been given to somebody.
//!
//! A bed is a **feature**, and features are static — built once from the map
//! and read from every thread ([`crate::sim::feature`]). Whose bed it is is
//! not: owners arrive and leave. So that fact lives here, on
//! [`GameState`](crate::sim::GameState), and is written only by the spawn pass:
//! arriving hands out the nearest bed nobody owns, leaving takes it back.
//!
//! **Nothing in a tick reads this.** A unit does not ask who owns what — it
//! remembers which bed is its own ([`Memory`](crate::sim::brain::Memory),
//! [`HOME_BED`](crate::sim::brain::memory::HOME_BED)), the way a person
//! knows, which is also why the crowd does not get a shared table to contend
//! over. This is only the bookkeeping that stops two arrivals being handed
//! the same bed.
//!
//! A `BTreeMap`, for the reason the brain's memory is one: an order that is a
//! property of the keys and never of a hasher.

use std::collections::BTreeMap;

use crate::map::Point;

use super::uid::Uid;

/// Which beds have an owner, and who.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Homes {
    owners: BTreeMap<Point, Uid>,
}

impl Homes {
    pub fn new() -> Homes {
        Homes::default()
    }

    /// Who owns `bed`, if anybody.
    pub fn owner(&self, bed: Point) -> Option<Uid> {
        self.owners.get(&bed).copied()
    }

    pub fn is_owned(&self, bed: Point) -> bool {
        self.owners.contains_key(&bed)
    }

    /// Give `bed` to `uid`. `false`, and nothing changes, if it already has an
    /// owner: **a bed is only ever given away while it is free.**
    pub fn claim(&mut self, bed: Point, uid: Uid) -> bool {
        if self.is_owned(bed) {
            return false;
        }
        self.owners.insert(bed, uid);
        true
    }

    /// Take back whatever `uid` owned, and say which bed it was. A scan of the
    /// owners, which is a scan of the beds — once per despawn, never per tick.
    pub fn release(&mut self, uid: Uid) -> Option<Point> {
        let bed = self.owners.iter().find(|&(_, &owner)| owner == uid).map(|(&bed, _)| bed)?;
        self.owners.remove(&bed);
        Some(bed)
    }

    /// How many beds have an owner.
    pub fn len(&self) -> usize {
        self.owners.len()
    }

    pub fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::uid::EntityType;

    fn human(n: u64) -> Uid {
        Uid::new(EntityType::Human, n)
    }

    #[test]
    fn a_bed_nobody_owns_can_be_given_to_somebody_and_says_whose_it_is() {
        let mut homes = Homes::new();
        let bed = Point::new(3, 4);
        assert_eq!(homes.owner(bed), None);
        assert!(homes.claim(bed, human(1)));
        assert_eq!(homes.owner(bed), Some(human(1)));
        assert!(homes.is_owned(bed));
        assert_eq!(homes.len(), 1);
    }

    #[test]
    fn a_bed_that_is_owned_is_not_given_away_again() {
        let mut homes = Homes::new();
        let bed = Point::new(3, 4);
        assert!(homes.claim(bed, human(1)));
        assert!(!homes.claim(bed, human(2)), "it is somebody else's");
        assert_eq!(homes.owner(bed), Some(human(1)), "and stays theirs");
    }

    #[test]
    fn leaving_gives_the_bed_back_to_be_given_to_the_next_arrival() {
        let mut homes = Homes::new();
        let bed = Point::new(3, 4);
        homes.claim(bed, human(1));
        assert_eq!(homes.release(human(1)), Some(bed));
        assert!(!homes.is_owned(bed));
        assert!(homes.claim(bed, human(2)));
        assert_eq!(homes.owner(bed), Some(human(2)));
    }

    #[test]
    fn releasing_somebody_who_owned_nothing_changes_nothing() {
        let mut homes = Homes::new();
        homes.claim(Point::new(1, 1), human(1));
        assert_eq!(homes.release(human(9)), None);
        assert_eq!(homes.len(), 1);
        assert!(!Homes::new().release(human(1)).is_some());
    }
}
