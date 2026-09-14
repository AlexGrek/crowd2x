//! The simulation's only randomness during a tick: one stream per entity per
//! tick, derived rather than stored.
//!
//! Out of `kinds.rs` because more than one thing wants it now — the walker
//! used to be the only thing that rolled dice, and a goal choosing somewhere
//! to wander is a second. Keeping one definition is what keeps the two
//! agreeing about what "the same entity on the same tick" means.

use rand::rngs::SmallRng;
use rand::SeedableRng;

use super::uid::Uid;

/// A deterministic RNG for one entity on one tick.
///
/// Per-entity and seeded, so the simulation replays identically from the same
/// starting state — and, more usefully, so the think phase needs no shared
/// mutable RNG, which is the one thing that would stop it parallelising.
pub(crate) fn tick_rng(uid: Uid, tick: u64) -> SmallRng {
    // Mixed rather than added: neighbouring ids on the same tick must not get
    // correlated streams, or a crowd wanders in formation.
    SmallRng::seed_from_u64(mix(uid.raw() ^ mix(tick)))
}

/// SplitMix64's finaliser: cheap, and good enough to decorrelate a counter.
pub(crate) fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::uid::EntityType;
    use rand::RngExt;

    #[test]
    fn the_same_entity_on_the_same_tick_rolls_the_same_dice() {
        let uid = Uid::new(EntityType::Human, 5);
        let a: u64 = tick_rng(uid, 9).random();
        let b: u64 = tick_rng(uid, 9).random();
        assert_eq!(a, b);
    }

    #[test]
    fn neighbouring_ids_on_one_tick_get_unrelated_streams() {
        // Adjacent ids and adjacent ticks are the inputs a crowd actually
        // produces, and the ones an unmixed seed would correlate.
        let a: u64 = tick_rng(Uid::new(EntityType::Human, 1), 3).random();
        let b: u64 = tick_rng(Uid::new(EntityType::Human, 2), 3).random();
        assert_ne!(a, b);
    }
}
