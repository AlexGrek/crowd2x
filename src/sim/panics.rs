//! [`Panics`]: who was seen running in a panic this tick.
//!
//! **Panic spreads by sight**: somebody outside the walls who sees somebody
//! else running for their life runs too (`brain::alarm`). What a look turns
//! up is a list of uids ([`Sightings`](super::brain::perception::Sightings)),
//! and nothing about a uid says whether it is panicking — asking the entity
//! would be one brain reading another's state mid-round, and a scan of the
//! crowd besides. So the move step, which already has every mover's body in
//! cache, writes down here every one that is panicking (one bool read each),
//! and react, after it, asks this list with a binary search.
//!
//! Rebuilt every tick before it is read, so **never saved**: a loaded world's
//! first move step writes it exactly as the saved world's would have.

use super::uid::Uid;

#[derive(Clone, Debug, Default)]
pub struct Panics {
    /// Sorted once the move step is done with it.
    seen: Vec<Uid>,
}

impl Panics {
    /// Forget last tick's: the move step is about to write this one's.
    pub fn clear(&mut self) {
        self.seen.clear();
    }

    /// `uid` was seen running in a panic. Grows only when more panic at once
    /// than ever before, keeping the capacity.
    pub fn push(&mut self, uid: Uid) {
        self.seen.push(uid);
    }

    /// Sort, ready to be asked. Uids are unique, so the order is the same on
    /// every run however they were pushed.
    pub fn finish(&mut self) {
        self.seen.sort_unstable();
    }

    /// Whether `uid` was seen running in a panic this tick.
    pub fn contains(&self, uid: Uid) -> bool {
        self.seen.binary_search(&uid).is_ok()
    }

    /// Say whether `uid` is panicking, keeping the list sorted: for a world
    /// of one (`testing::World`), which has no move step over a crowd.
    pub fn set(&mut self, uid: Uid, panicking: bool) {
        match (self.seen.binary_search(&uid), panicking) {
            (Err(at), true) => self.seen.insert(at, uid),
            (Ok(at), false) => {
                self.seen.remove(at);
            }
            _ => {}
        }
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::uid::EntityType;

    #[test]
    fn whoever_was_pushed_is_found_once_sorted_and_nobody_else() {
        let mut panics = Panics::default();
        let uids: Vec<Uid> = [9, 3, 7].map(|n| Uid::new(EntityType::Human, n)).to_vec();
        for &uid in &uids {
            panics.push(uid);
        }
        panics.finish();
        assert!(uids.iter().all(|&uid| panics.contains(uid)));
        assert!(!panics.contains(Uid::new(EntityType::Human, 4)));
        panics.clear();
        assert!(panics.is_empty());
    }

    #[test]
    fn setting_one_keeps_the_rest_sorted() {
        let mut panics = Panics::default();
        let (a, b, c) = (Uid::new(EntityType::Human, 5), Uid::new(EntityType::Human, 1), Uid::new(EntityType::Human, 3));
        panics.set(a, true);
        panics.set(b, true);
        panics.set(c, true);
        panics.set(a, true);
        assert_eq!(panics.len(), 3);
        panics.set(c, false);
        assert!(panics.contains(a) && panics.contains(b) && !panics.contains(c));
    }
}
