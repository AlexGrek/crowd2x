//! What a person's body and mind are doing: [`Stats`].
//!
//! Read by [`super::kinds::Human::think`] — the first thinking process, and so
//! far the only one — the same way [`super::entity::Think`] hands it the map
//! and the crowd. Nothing here decides anything; a stat is a number a brain
//! reads, not a brain of its own.
//!
//! One field per stat, not a map by name: a `Human` always has exactly this
//! set of stats, so indexing by name would trade a compile error (a typo in a
//! field) for a runtime one (a typo in a string). Adding a stat is three
//! small edits, all in this file — a field, a line in [`Stats::random`], and
//! a getter — and nothing outside it changes, because [`Stats::fields`] is
//! the one place that turns the struct into the name/value pairs a debug view
//! wants.

use rand::rngs::SmallRng;
use rand::RngExt;

/// A person's needs and condition. Every field is on a 0-100 scale except
/// [`Stats::attention`], which is 0-1 — see each getter for what the ends
/// mean.
///
/// Read-only from outside this module: nothing but [`Stats::random`] sets one
/// yet, because nothing changes them over time yet. That is the next thing to
/// build, not this one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    health: f32,
    stamina: f32,
    fun: f32,
    hunger: f32,
    bladder: f32,
    mental_health: f32,
    attention: f32,
}

impl Stats {
    /// Every stat at a random value inside its allowed range — nobody is born
    /// fully rested, fed and content, and nobody is born at zero either.
    pub fn random(rng: &mut SmallRng) -> Stats {
        Stats {
            health: rng.random_range(0.0..=100.0),
            stamina: rng.random_range(0.0..=100.0),
            fun: rng.random_range(0.0..=100.0),
            hunger: rng.random_range(0.0..=100.0),
            bladder: rng.random_range(0.0..=100.0),
            mental_health: rng.random_range(0.0..=100.0),
            attention: rng.random_range(0.0..=1.0),
        }
    }

    /// 0 (dead) to 100 (uninjured).
    pub fn health(&self) -> f32 {
        self.health
    }

    /// 0 (exhausted) to 100 (fully rested).
    pub fn stamina(&self) -> f32 {
        self.stamina
    }

    /// 0 (bored) to 100 (having a great time).
    pub fn fun(&self) -> f32 {
        self.fun
    }

    /// 0 (full) to 100 (starving).
    pub fn hunger(&self) -> f32 {
        self.hunger
    }

    /// 0 (empty) to 100 (desperate).
    pub fn bladder(&self) -> f32 {
        self.bladder
    }

    /// 0 (in crisis) to 100 (thriving).
    pub fn mental_health(&self) -> f32 {
        self.mental_health
    }

    /// 0 (not paying attention at all) to 1 (fully focused).
    pub fn attention(&self) -> f32 {
        self.attention
    }

    /// Name and value of every stat, in the order above — the shape
    /// [`super::entity::GameEntity::debug_fields`] wants, so a kind that
    /// carries stats can hand them over in one line rather than naming each
    /// field again at the call site.
    pub fn fields(&self) -> Vec<(&'static str, f32)> {
        vec![
            ("health", self.health),
            ("stamina", self.stamina),
            ("fun", self.fun),
            ("hunger", self.hunger),
            ("bladder", self.bladder),
            ("mental_health", self.mental_health),
            ("attention", self.attention),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn a_random_person_is_within_every_stat_s_allowed_range() {
        let mut rng = SmallRng::seed_from_u64(1);
        for _ in 0..1000 {
            let stats = Stats::random(&mut rng);
            assert!((0.0..=100.0).contains(&stats.health()));
            assert!((0.0..=100.0).contains(&stats.stamina()));
            assert!((0.0..=100.0).contains(&stats.fun()));
            assert!((0.0..=100.0).contains(&stats.hunger()));
            assert!((0.0..=100.0).contains(&stats.bladder()));
            assert!((0.0..=100.0).contains(&stats.mental_health()));
            assert!((0.0..=1.0).contains(&stats.attention()));
        }
    }

    #[test]
    fn two_random_people_are_not_carbon_copies() {
        let mut rng = SmallRng::seed_from_u64(2);
        let a = Stats::random(&mut rng);
        let b = Stats::random(&mut rng);
        assert_ne!(a, b);
    }

    #[test]
    fn fields_are_named_in_declaration_order() {
        let mut rng = SmallRng::seed_from_u64(3);
        let stats = Stats::random(&mut rng);
        let names: Vec<&str> = stats.fields().iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            ["health", "stamina", "fun", "hunger", "bladder", "mental_health", "attention"]
        );
    }
}
