//! What a person's body and mind are like right now: [`Stats`].
//!
//! Read by a human's brain — its routines turn a stat into how badly a goal is
//! wanted. **Written only by a [`Process`](super::Process)**: the setters
//! below are visible to `biology` and nothing outside it, so a task that feeds
//! somebody says what happened ([`Event`](super::Event)) and the processes
//! decide what that does to the numbers. Nothing here decides anything; a stat
//! is a number, not a mechanism.
//!
//! One field per stat, not a map by name: a `Human` always has exactly this
//! set of stats, so indexing by name would trade a compile error (a typo in a
//! field) for a runtime one (a typo in a string). Adding a stat is a field, a
//! line in [`Stats::random`], a getter and a line in [`Stats::fields`] — the
//! one place that turns the struct into the name/value pairs a debug view
//! wants.

use rand::rngs::SmallRng;
use rand::RngExt;

/// A person's needs and condition. Every field is on a 0-100 scale except
/// [`Stats::attention`], which is 0-1 — see each getter for what the ends
/// mean.
///
/// Hunger, thirst, bladder and fun are the stats that move so far — the rest
/// are rolled at spawn and stay put until a process wants them to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    health: f32,
    stamina: f32,
    fun: f32,
    hunger: f32,
    thirst: f32,
    bladder: f32,
    mental_health: f32,
    attention: f32,
}

/// A need kept to its 0-100 range.
fn need(value: f32) -> f32 {
    value.clamp(0.0, 100.0)
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
            thirst: rng.random_range(0.0..=100.0),
            bladder: rng.random_range(0.0..=100.0),
            mental_health: rng.random_range(0.0..=100.0),
            attention: rng.random_range(0.0..=1.0),
        }
    }

    /// `by` more hunger (less, for a negative `by`), never past its range.
    pub(super) fn change_hunger(&mut self, by: f32) {
        self.hunger = need(self.hunger + by);
    }

    pub(super) fn change_thirst(&mut self, by: f32) {
        self.thirst = need(self.thirst + by);
    }

    pub(super) fn change_bladder(&mut self, by: f32) {
        self.bladder = need(self.bladder + by);
    }

    pub(super) fn change_fun(&mut self, by: f32) {
        self.fun = need(self.fun + by);
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
            thirst: 50.0,
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

    #[cfg(test)]
    pub(crate) fn with_thirst(mut self, thirst: f32) -> Stats {
        self.thirst = thirst;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_bladder(mut self, bladder: f32) -> Stats {
        self.bladder = bladder;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_fun(mut self, fun: f32) -> Stats {
        self.fun = fun;
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

    /// 0 (quenched) to 100 (parched).
    pub fn thirst(&self) -> f32 {
        self.thirst
    }

    /// 0 (empty) to 100 (desperate).
    pub fn bladder(&self) -> f32 {
        self.bladder
    }

    /// 0 (having a great time) to 100 (bored): [`Stats::fun`] read as a need.
    ///
    /// Every other need *rises* towards the thing that has to be done about
    /// it — hunger, thirst, a bladder — and fun is the one stat that drains
    /// instead. Turning it over here rather than in the routine that watches
    /// it is what keeps every need on one scale, so how badly somebody wants
    /// a go on the computer can be compared with how badly they want a meal
    /// (`NeedRoutine`). Derived, so it cannot disagree with the stat it is
    /// the other side of.
    pub fn boredom(&self) -> f32 {
        100.0 - self.fun
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
    /// [`crate::sim::GameEntity::debug_fields`] wants, so a kind that carries
    /// stats can hand them over in one line rather than naming each field
    /// again at the call site.
    pub fn fields(&self) -> Vec<(&'static str, f32)> {
        vec![
            ("health", self.health),
            ("stamina", self.stamina),
            ("fun", self.fun),
            ("hunger", self.hunger),
            ("thirst", self.thirst),
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
            assert!((0.0..=100.0).contains(&stats.thirst()));
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
    fn a_need_never_leaves_its_range_however_far_it_is_pushed() {
        let mut stats = Stats::calm();
        stats.change_hunger(1000.0);
        stats.change_thirst(-1000.0);
        stats.change_bladder(1000.0);
        stats.change_fun(-1000.0);
        assert_eq!((stats.hunger(), stats.thirst(), stats.bladder()), (100.0, 0.0, 100.0));
        assert_eq!(stats.fun(), 0.0);
    }

    #[test]
    fn boredom_is_fun_the_other_way_up() {
        assert_eq!(
            Stats::calm().with_fun(100.0).boredom(),
            0.0,
            "having a great time is not being bored"
        );
        assert_eq!(Stats::calm().with_fun(0.0).boredom(), 100.0);
        assert_eq!(Stats::calm().with_fun(30.0).boredom(), 70.0);
    }

    #[test]
    fn fields_are_named_in_declaration_order() {
        let mut rng = SmallRng::seed_from_u64(3);
        let stats = Stats::random(&mut rng);
        let names: Vec<&str> = stats.fields().iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            ["health", "stamina", "fun", "hunger", "thirst", "bladder", "mental_health", "attention"]
        );
    }
}
