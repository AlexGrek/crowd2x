//! What a unit makes of what it sees: [`Attention`].
//!
//! [`super::perception`] says who is in view; this decides whether any of them
//! is *news*. It keeps a short **vision memory** — the faces seen lately, each
//! with how long it has been out of sight — and a sighting of somebody already
//! in it is nothing: they are refreshed and that is all. Somebody not in it is
//! noticed, remembered, and reacted to ([`Notice`]), so walking past the same
//! person twice in a minute is one meeting and not two.
//!
//! **Memory fades quickly.** A face out of sight for [`FORGET_AFTER`] of world
//! time is forgotten, and seeing it again after that is a meeting again, as
//! good as the first. One that stays in view, or keeps coming back into it,
//! stays in mind.
//!
//! The memory is a fixed array of [`FACES`]: when it is full, whoever has been
//! out of sight longest makes room. Boxed once, at spawn, for the reason
//! [`Perception`](super::Perception)'s sightings are — touched every 7th tick,
//! it has no business in the per-tick hot data — and nothing allocated after
//! that. It runs when perception does — on the unit's own beat, every 7th tick —
//! handed the world time since it last ran; and while the unit is asleep it
//! runs with nothing seen, so faces fade overnight.
//!
//! What a meeting does to the unit is the brain's business, not this
//! module's: [`Attention::take_in`] hands each one to a closure.

use crate::sim::clock::MINUTE;
use crate::sim::uid::Uid;

use super::perception::{Sighting, SIGHTINGS};

/// World time out of sight after which a face is forgotten. Quickly: half an
/// hour, fifteen seconds of watching.
pub const FORGET_AFTER: f32 = 30.0 * MINUTE;

/// How many faces a unit keeps in mind at once. More than one look's worth
/// ([`SIGHTINGS`]), so a look never pushes out somebody it also saw.
pub const FACES: usize = 32;

const _: () = assert!(FACES > SIGHTINGS);

/// The faces in mind, as two arrays rather than an array of pairs: what a
/// look does is scan every key for one, and age every timer at once, and both
/// are a tight loop over one contiguous array that way.
///
/// **Small on purpose, because it is cold.** A unit's faces were last touched
/// a look ago, seven ticks and a crowd's worth of other units back, so in a
/// big crowd every look reads them from far away in memory — and the cost of
/// a look is mostly how many cache lines that is, not what is done with them.
/// So a face is a 32-bit key ([`key`]) and a 16-bit timer, and all of it is
/// three lines instead of six.
///
/// An empty slot is key `0` — [`key`] never gives it — with the timer at its
/// top ([`EMPTY`]), so "the slot to reuse" is one argmax over the timers: an
/// empty one wins over anybody, and a full memory gives up whoever has been
/// out of sight longest.
#[derive(Clone, PartialEq, Debug)]
struct Faces {
    keys: [u32; FACES],
    /// Quarter world seconds since each was last seen, saturating.
    unseen: [u16; FACES],
    /// How many people it has been glad to see, ever — a meeting being a
    /// person noticed who was not in mind. For the debug menu and tests, in
    /// place of a log line per meeting, which in a crowd would be most of the
    /// log and an allocation on most looks. In the box, beside the faces it
    /// counts, because it is touched exactly when they are.
    met: u32,
}

/// What a timer counts in a world second. A quarter second is finer than any
/// look is apart (a look is ~13 world seconds) and leaves [`FORGET_AFTER`]
/// well inside 16 bits.
const PER_SECOND: f32 = 4.0;

/// [`FORGET_AFTER`] on a timer.
const FORGOTTEN: u16 = (FORGET_AFTER * PER_SECOND) as u16;

/// The timer of an empty slot: longer out of sight than anybody.
const EMPTY: u16 = u16::MAX;

const _: () = assert!((FORGOTTEN as u32) < EMPTY as u32);

/// A face's key: the id folded to 32 bits, never `0`.
///
/// Two people whose keys collide are one face to whoever sees both — the
/// second is not news while the first is in mind. One pair in four billion,
/// for a missed greeting; the price of three cache lines a look instead of
/// six.
fn key(uid: Uid) -> u32 {
    let raw = uid.raw();
    ((raw ^ (raw >> 32)) as u32).max(1)
}

impl Default for Faces {
    fn default() -> Faces {
        Faces { keys: [0; FACES], unseen: [EMPTY; FACES], met: 0 }
    }
}

/// Somebody seen who was not in mind: what a reaction is about.
pub type Notice = Sighting;

/// A unit's vision memory.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Attention {
    faces: Box<Faces>,
}

impl Attention {
    /// `elapsed` **world** seconds go by for every face, and those gone too
    /// long are forgotten. All a unit with its eyes shut does.
    ///
    /// Apart from [`Attention::take_in`], and asked first — before the look —
    /// for speed: the faces were last touched a look ago and are the one part
    /// of looking round likely to come from far away in memory, so touching
    /// them before the scan lets them arrive while it runs instead of after.
    pub fn age(&mut self, elapsed: f32) {
        let Faces { keys, unseen, .. } = &mut *self.faces;
        let gone = (elapsed * PER_SECOND).round().clamp(0.0, EMPTY as f32) as u16;
        for (key, unseen) in keys.iter_mut().zip(unseen.iter_mut()) {
            *unseen = unseen.saturating_add(gone);
            if *unseen > FORGOTTEN {
                *key = 0;
                *unseen = EMPTY;
            }
        }
    }

    /// Take in a look, once [`Attention::age`] has been told how long since
    /// the last one: everyone `seen` is refreshed — and everyone seen who was
    /// not in mind is remembered and handed to `notice`, in the order they
    /// were seen.
    pub fn take_in(&mut self, seen: impl Iterator<Item = Sighting>, mut notice: impl FnMut(Notice)) {
        let Faces { keys, unseen, .. } = &mut *self.faces;
        for sighting in seen {
            let wanted = key(sighting.uid);
            if let Some(i) = keys.iter().position(|&key| key == wanted) {
                unseen[i] = 0;
                continue;
            }
            let room = longest_unseen(unseen);
            keys[room] = wanted;
            unseen[room] = 0;
            notice(sighting);
        }
    }

    /// Count `n` more meetings.
    pub fn met_with(&mut self, n: u32) {
        self.faces.met = self.faces.met.saturating_add(n);
    }

    /// How many people it has been glad to see, ever.
    pub fn met(&self) -> u32 {
        self.faces.met
    }

    /// Whether `uid` is in mind.
    pub fn remembers(&self, uid: Uid) -> bool {
        self.faces.keys.contains(&key(uid))
    }

    /// How many faces are in mind.
    pub fn len(&self) -> usize {
        self.faces.keys.iter().filter(|&&key| key != 0).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// For the brains menu. Allocates; never asked in a tick. One field, the
    /// count in its value, so the menu's alignment cannot split the two.
    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let faces = match self.len() {
            0 => "nobody".to_string(),
            1 => "1 face".to_string(),
            len => format!("{len} faces"),
        };
        vec![("in mind", format!("{faces}, met {}", self.faces.met))]
    }
}

/// The slot out of sight the longest — an empty one first, since it is
/// infinitely long — ties to the first, so which goes is the same on every run.
fn longest_unseen(unseen: &[u16; FACES]) -> usize {
    let mut longest = 0;
    for (i, &time) in unseen.iter().enumerate().skip(1) {
        if time > unseen[longest] {
            longest = i;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Point;
    use crate::sim::brain::perception::Range;
    use crate::sim::uid::EntityType;

    fn sighting(n: u64) -> Sighting {
        Sighting { uid: Uid::new(EntityType::Human, n), cell: Point::new(0, 0), range: Range::Far }
    }

    /// One look, returning who was noticed.
    fn attend(attention: &mut Attention, seen: &[u64], elapsed: f32) -> Vec<u64> {
        let mut noticed = Vec::new();
        attention.age(elapsed);
        attention.take_in(seen.iter().map(|&n| sighting(n)), |notice| noticed.push(notice.uid.body()));
        noticed
    }

    #[test]
    fn somebody_new_is_noticed_once_and_not_again_while_in_mind() {
        let mut attention = Attention::default();
        assert_eq!(attend(&mut attention, &[2, 3], 0.0), [2, 3]);
        assert_eq!(attend(&mut attention, &[2, 3], MINUTE), Vec::<u64>::new());
        assert_eq!(attend(&mut attention, &[3, 4], MINUTE), [4]);
        assert!(attention.remembers(Uid::new(EntityType::Human, 2)));
    }

    #[test]
    fn a_face_out_of_sight_long_enough_is_forgotten_and_met_again() {
        let mut attention = Attention::default();
        attend(&mut attention, &[2], 0.0);
        attend(&mut attention, &[], FORGET_AFTER * 0.9);
        assert!(attention.remembers(Uid::new(EntityType::Human, 2)), "not yet");
        assert_eq!(attend(&mut attention, &[], FORGET_AFTER * 0.2), Vec::<u64>::new());
        assert!(attention.is_empty(), "gone");
        assert_eq!(attend(&mut attention, &[2], MINUTE), [2], "and a meeting again");
    }

    #[test]
    fn a_face_that_keeps_coming_back_into_view_stays_in_mind() {
        let mut attention = Attention::default();
        attend(&mut attention, &[2], 0.0);
        for _ in 0..10 {
            attend(&mut attention, &[], FORGET_AFTER * 0.5);
            assert_eq!(attend(&mut attention, &[2], FORGET_AFTER * 0.4), Vec::<u64>::new());
        }
    }

    #[test]
    fn a_face_freed_by_forgetting_is_the_first_slot_reused() {
        let mut attention = Attention::default();
        attend(&mut attention, &[1, 2, 3], 0.0);
        attend(&mut attention, &[1, 3], FORGET_AFTER * 0.6);
        attend(&mut attention, &[1, 3], FORGET_AFTER * 0.6);
        assert!(!attention.remembers(Uid::new(EntityType::Human, 2)));
        assert_eq!(attend(&mut attention, &[4], 1.0), [4]);
        assert_eq!(attention.faces.keys[1], key(Uid::new(EntityType::Human, 4)));
        assert_eq!(attention.len(), 3);
    }

    #[test]
    fn the_debug_view_says_who_is_in_mind_and_how_many_were_met() {
        let mut attention = Attention::default();
        assert_eq!(attention.debug_fields(), [("in mind", "nobody, met 0".to_string())]);
        attend(&mut attention, &[2, 3], 0.0);
        attention.met_with(2);
        assert_eq!(attention.debug_fields(), [("in mind", "2 faces, met 2".to_string())]);
    }

    #[test]
    fn a_key_is_never_the_empty_slot_and_tells_ids_apart() {
        let keys: Vec<u32> = (1..1000).map(|n| key(Uid::new(EntityType::Human, n))).collect();
        assert!(keys.iter().all(|&key| key != 0));
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
        assert_ne!(key(Uid::new(EntityType::Human, 7)), key(Uid::new(EntityType::Dog, 7)));
    }

    #[test]
    fn a_full_memory_makes_room_by_forgetting_who_was_seen_longest_ago() {
        let mut attention = Attention::default();
        attend(&mut attention, &[1], 0.0);
        for n in 2..=FACES as u64 {
            attend(&mut attention, &[n], 10.0);
        }
        assert_eq!(attention.len(), FACES);
        assert_eq!(attend(&mut attention, &[100], 10.0), [100]);
        assert_eq!(attention.len(), FACES);
        assert!(!attention.remembers(Uid::new(EntityType::Human, 1)), "the oldest went");
        assert!(attention.remembers(Uid::new(EntityType::Human, 2)));
    }
}
