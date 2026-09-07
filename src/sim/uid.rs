//! The simulation's own entity id.
//!
//! Deliberately **not** Bevy's `Entity`. An actor exists whether or not
//! anything is drawing it, and most of them will not be: entities are for
//! things that draw, so a sprite comes and goes as an actor walks on and off
//! the canvas while its identity has to survive that. A `Uid` also outlives a
//! Bevy `World` — it can be written to a save file, quoted in a log line, and
//! compared across a reload, none of which an `Entity` index can do.
//!
//! # Layout
//!
//! 64 bits: the top byte is the [`EntityType`], the low 56 are random.
//!
//! ```text
//!  63          56 55                                                  0
//! +--------------+----------------------------------------------------+
//! |     type     |                   random, never 0                   |
//! +--------------+----------------------------------------------------+
//! ```
//!
//! The type in the id means a log line, a crash dump or a debugger watch
//! window says *what* an actor is without a lookup into a state that may
//! already have dropped it. It also makes an id from the wrong pool obvious
//! rather than merely wrong.
//!
//! Type discriminant `0` is never used, so the low bits being non-zero is not
//! actually load-bearing for `NonZeroU64` — but both halves are kept non-zero
//! anyway so that a zeroed word is not a valid id under any future type.

use std::fmt;
use std::num::NonZeroU64;

/// How many low bits are the random part.
const RANDOM_BITS: u32 = 56;

/// Mask of the random part.
pub(crate) const RANDOM_MASK: u64 = (1 << RANDOM_BITS) - 1;

/// What an entity is. The discriminant is the top byte of every [`Uid`] of
/// that kind, so it is part of the id format: renumbering one invalidates
/// every id already written down.
///
/// `0` is deliberately unused — see the module docs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum EntityType {
    Human = 1,
    Dog = 2,
}

impl EntityType {
    pub const ALL: [EntityType; 2] = [EntityType::Human, EntityType::Dog];

    /// The name used in logs and in a QA script's `spawn` step.
    pub const fn name(self) -> &'static str {
        match self {
            EntityType::Human => "human",
            EntityType::Dog => "dog",
        }
    }

    pub fn from_name(name: &str) -> Option<EntityType> {
        EntityType::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The inverse of the discriminant cast.
    ///
    /// `None` rather than a panic for a byte this build has no type for: the
    /// same rule [`crate::map::TerrainId::terrain`] follows for a tile id from
    /// a newer version. An id is data, and data can arrive from a file.
    pub const fn from_tag(tag: u8) -> Option<EntityType> {
        match tag {
            1 => Some(EntityType::Human),
            2 => Some(EntityType::Dog),
            _ => None,
        }
    }
}

impl fmt::Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A unique, randomised, type-tagged entity id.
///
/// `NonZeroU64` so `Option<Uid>` is the same size as a `Uid` — worth having,
/// since a great many things hold "the entity I am following, if any".
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uid(NonZeroU64);

impl Uid {
    /// Build an id from a type and a random 56-bit body.
    ///
    /// Bits above [`RANDOM_MASK`] in `random` are discarded rather than
    /// rejected — the caller is an RNG handing over a full word, and masking
    /// here means there is one place that knows the layout.
    pub(crate) fn new(kind: EntityType, random: u64) -> Uid {
        // Zero would make a valid `NonZeroU64` anyway once the tag is on, but
        // an all-zero body is the one value a broken RNG produces forever, so
        // it is worth not minting a stream of colliding ids from it.
        let body = (random & RANDOM_MASK).max(1);
        let bits = ((kind as u64) << RANDOM_BITS) | body;
        Uid(NonZeroU64::new(bits).expect("tag is non-zero"))
    }

    /// What kind of entity this is, from the id alone.
    ///
    /// `None` for a tag this build does not know.
    pub fn kind(self) -> Option<EntityType> {
        EntityType::from_tag((self.0.get() >> RANDOM_BITS) as u8)
    }

    /// The whole 64-bit value, for serialising or hashing.
    pub fn raw(self) -> u64 {
        self.0.get()
    }

    /// The random half, without the type tag.
    pub fn body(self) -> u64 {
        self.0.get() & RANDOM_MASK
    }

    /// Rebuild an id from [`Uid::raw`]. `None` for zero.
    ///
    /// Does *not* check the tag: an id from a newer build naming a type this
    /// one has never heard of is still that entity's id, and round-tripping it
    /// unchanged is better than dropping it. [`Uid::kind`] is where the
    /// unknown surfaces.
    pub fn from_raw(bits: u64) -> Option<Uid> {
        NonZeroU64::new(bits).map(Uid)
    }
}

/// `human:3f1c9a20b4e7d5` — the type, then the body in hex.
///
/// The type is spelled out rather than left as a tag byte because the whole
/// reason it is in the id is to be readable at the point it is printed.
impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            Some(kind) => write!(f, "{kind}:{:014x}", self.body()),
            None => write!(f, "?{:02x}:{:014x}", self.0.get() >> RANDOM_BITS, self.body()),
        }
    }
}

impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uid({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_type_survives_the_round_trip_through_the_bits() {
        for kind in EntityType::ALL {
            let uid = Uid::new(kind, 0x00ff_eedd_ccbb_aa99);
            assert_eq!(uid.kind(), Some(kind));
            assert_eq!(Uid::from_raw(uid.raw()), Some(uid));
        }
    }

    #[test]
    fn the_body_is_the_low_56_bits_and_the_tag_never_reaches_it() {
        // A random word wider than the body must not spill into the tag, or
        // one unlucky draw silently changes an entity's type.
        let uid = Uid::new(EntityType::Dog, u64::MAX);
        assert_eq!(uid.kind(), Some(EntityType::Dog));
        assert_eq!(uid.body(), RANDOM_MASK);
        assert_eq!(uid.raw() >> RANDOM_BITS, EntityType::Dog as u64);
    }

    #[test]
    fn an_all_zero_body_still_makes_a_usable_id() {
        let uid = Uid::new(EntityType::Human, 0);
        assert_eq!(uid.kind(), Some(EntityType::Human));
        assert_ne!(uid.body(), 0);
    }

    #[test]
    fn an_unknown_type_tag_reads_as_unknown_rather_than_panicking() {
        // An id minted by a build that has a type this one does not.
        let future = Uid::from_raw((99 << RANDOM_BITS) | 0x1234).expect("non-zero");
        assert_eq!(future.kind(), None);
        // ...and it still round-trips, so passing it through loses nothing.
        assert_eq!(Uid::from_raw(future.raw()), Some(future));
    }

    #[test]
    fn zero_is_not_an_id() {
        assert_eq!(Uid::from_raw(0), None);
        assert_eq!(
            std::mem::size_of::<Option<Uid>>(),
            std::mem::size_of::<Uid>()
        );
    }

    #[test]
    fn an_id_prints_its_type() {
        let uid = Uid::new(EntityType::Human, 0xab_cdef);
        assert_eq!(uid.to_string(), "human:00000000abcdef");
    }

    #[test]
    fn type_names_round_trip() {
        for kind in EntityType::ALL {
            assert_eq!(EntityType::from_name(kind.name()), Some(kind));
        }
        assert_eq!(EntityType::from_name("dragon"), None);
    }
}
