//! The simulation's only randomness during a tick: one stream per entity per
//! tick, derived rather than stored.
//!
//! Out of `kinds.rs` because more than one thing wants it now — the walker
//! used to be the only thing that rolled dice, and a goal choosing somewhere
//! to wander is a second. Keeping one definition is what keeps the two
//! agreeing about what "the same entity on the same tick" means.

use rand::rngs::{SmallRng, Xoshiro256PlusPlus};
use rand::SeedableRng;

use crate::save::{invalid, Invalid};

use super::uid::Uid;

/// The world's own stream: what mints ids and seeds each new entity
/// (`GameState::rng`).
///
/// Named rather than left as `SmallRng`, because a save keeps its state
/// ([`state_of`]) and `SmallRng` is whichever generator suits the platform —
/// on a 64-bit one it *is* xoshiro256++, seeded the same way, so naming it
/// changed nothing a world does, and a save from one machine means the same
/// on another.
pub(crate) type WorldRng = Xoshiro256PlusPlus;

/// The shape `rand` serialises xoshiro256++ as: its four words.
#[derive(serde::Serialize, serde::Deserialize)]
struct Words {
    s: [u64; 4],
}

/// The generator's state, four words, for a save. Read through `rand`'s own
/// serialisation — it has no other door — once a save, so the detour costs
/// nothing that matters.
pub(crate) fn state_of(rng: &WorldRng) -> [u64; 4] {
    let value = serde_json::to_value(rng).expect("an rng's state serialises");
    let words: Words = serde_json::from_value(value).expect("xoshiro256++ is four words");
    words.s
}

/// The generator [`state_of`] read, carrying on exactly where it left off.
/// All zeros is refused: xoshiro never reaches it, so it is a broken save.
pub(crate) fn from_state(words: &[u64]) -> Result<WorldRng, Invalid> {
    let Ok(s) = <[u64; 4]>::try_from(words) else {
        return invalid(format!("the world's rng is {} words, not 4", words.len()));
    };
    if s == [0; 4] {
        return invalid("the world's rng is all zeros");
    }
    let value = serde_json::to_value(Words { s }).expect("four words serialise");
    Ok(serde_json::from_value(value).expect("xoshiro256++ is four words"))
}

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
    fn the_world_rng_is_the_stream_small_rng_always_was() {
        // Naming the generator must not have changed a single world.
        let mut named = WorldRng::seed_from_u64(42);
        let mut small = SmallRng::seed_from_u64(42);
        for _ in 0..100 {
            assert_eq!(named.random::<u64>(), small.random::<u64>());
        }
    }

    #[test]
    fn a_saved_rng_carries_on_exactly_where_it_left_off() {
        let mut rng = WorldRng::seed_from_u64(7);
        for _ in 0..13 {
            let _: u64 = rng.random();
        }
        let mut restored = from_state(&state_of(&rng)).unwrap();
        for _ in 0..100 {
            assert_eq!(restored.random::<u64>(), rng.random::<u64>());
        }
    }

    #[test]
    fn a_broken_rng_state_is_refused() {
        assert!(from_state(&[1, 2, 3]).is_err());
        assert!(from_state(&[0; 4]).is_err());
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
