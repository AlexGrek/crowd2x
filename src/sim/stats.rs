//! What a person's body and mind are doing: [`Stats`].
//!
//! Read by a human's brain — its routines turn a stat into how badly a goal is
//! wanted, and a goal met changes one back ([`Stats::eat`]). Nothing here
//! decides anything; a stat is a number a brain reads, not a brain of its own.
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

/// Hunger gained per second. From fed to [`PECKISH`] in about a minute, which is
/// slow enough that a crowd is not always eating and fast enough that watching
/// one person for a minute shows a meal.
///
/// [`PECKISH`]: super::brain::routines::PECKISH
pub const HUNGER_PER_SECOND: f32 = 1.0;

/// A person's needs and condition. Every field is on a 0-100 scale except
/// [`Stats::attention`], which is 0-1 — see each getter for what the ends
/// mean.
///
/// Changed from outside this module only through what a body does: time passing
/// ([`Stats::metabolise`]) and a need being met ([`Stats::eat`]). Hunger is the
/// only stat that moves so far — the rest are rolled at spawn and stay put
/// until something wants them to.
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

    /// Time passing: `dt` seconds' worth of getting hungrier.
    pub fn metabolise(&mut self, dt: f32) {
        self.hunger = (self.hunger + HUNGER_PER_SECOND * dt).clamp(0.0, 100.0);
    }

    /// A meal: `amount` hunger gone, and never below full.
    pub fn eat(&mut self, amount: f32) {
        self.hunger = (self.hunger - amount).clamp(0.0, 100.0);
    }

    /// Every stat at the middle of its range — a person with nothing unusual
    /// about them, for a test that wants to set one stat and know the rest.
    #[cfg(test)]
    pub(crate) fn calm() -> Stats {
        Stats {
            health: 50.0,
            stamina: 50.0,
            fun: 50.0,
            hunger: 50.0,
            bladder: 50.0,
            mental_health: 50.0,
            attention: 0.5,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_hunger(mut self, hunger: f32) -> Stats {
        self.hunger = hunger;
        self
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
    fn time_makes_a_person_hungrier_and_never_past_starving() {
        let mut stats = Stats::calm().with_hunger(10.0);
        stats.metabolise(5.0);
        assert_eq!(stats.hunger(), 10.0 + 5.0 * HUNGER_PER_SECOND);
        stats.metabolise(10_000.0);
        assert_eq!(stats.hunger(), 100.0);
    }

    #[test]
    fn eating_takes_hunger_away_and_never_past_full() {
        let mut stats = Stats::calm().with_hunger(70.0);
        stats.eat(60.0);
        assert_eq!(stats.hunger(), 10.0);
        stats.eat(60.0);
        assert_eq!(stats.hunger(), 0.0);
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
