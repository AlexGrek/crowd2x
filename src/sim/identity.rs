//! Who someone is, beyond a [`super::biology::Stats`] a human has and a dog
//! does not: [`Identity`].
//!
//! Same shape as `Stats` on purpose — one struct, built once from a seeded
//! RNG at spawn ([`Identity::human`] or [`Identity::pet`]), read-only from
//! outside this module — because nothing here changes over the entity's life
//! either.
//!
//! [`Gender`] is rolled independently of [`Identity::name`]: `fake`'s
//! `name::en::FirstName` draws from one unsplit pool rather than a male and a
//! female list, so a name is not guaranteed to match the gender rolled beside
//! it. That is a property of the crate, not a bug to route around with a
//! second one — an entity's gender is cosmetic today and nothing reads it
//! yet.

use fake::Fake;
use fake::faker::name::en::{FirstName, LastName};
use rand::rngs::SmallRng;
use rand::RngExt;

/// Two genders, each as likely as the other at spawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gender {
    Male,
    Female,
}

impl Gender {
    fn random(rng: &mut SmallRng) -> Gender {
        if rng.random() {
            Gender::Male
        } else {
            Gender::Female
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Gender::Male => "male",
            Gender::Female => "female",
        }
    }
}

/// A person's name and gender, rolled once at spawn.
#[derive(Clone, PartialEq, Debug)]
pub struct Identity {
    name: String,
    gender: Gender,
}

impl Identity {
    /// A person: first and last name.
    pub fn human(rng: &mut SmallRng) -> Identity {
        let first: String = FirstName().fake_with_rng(rng);
        let last: String = LastName().fake_with_rng(rng);
        Identity {
            name: format!("{first} {last}"),
            gender: Gender::random(rng),
        }
    }

    /// A pet: a first name and nothing else — a dog has no surname to draw
    /// on, and `fake`'s pool has no separate one for animals. Drawn from the
    /// same list a person's first name comes from, which happens to already
    /// cover most names a dog plausibly has (Max, Bella, Charlie, Rosie).
    pub fn pet(rng: &mut SmallRng) -> Identity {
        Identity {
            name: FirstName().fake_with_rng(rng),
            gender: Gender::random(rng),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn gender(&self) -> Gender {
        self.gender
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn a_human_identity_has_a_first_and_a_last_name() {
        let mut rng = SmallRng::seed_from_u64(1);
        for _ in 0..1000 {
            let identity = Identity::human(&mut rng);
            assert_eq!(identity.name().split(' ').count(), 2);
        }
    }

    #[test]
    fn a_pet_identity_has_only_a_first_name() {
        let mut rng = SmallRng::seed_from_u64(4);
        for _ in 0..1000 {
            let identity = Identity::pet(&mut rng);
            assert_eq!(identity.name().split(' ').count(), 1);
        }
    }

    #[test]
    fn two_random_identities_are_not_carbon_copies() {
        let mut rng = SmallRng::seed_from_u64(2);
        let a = Identity::human(&mut rng);
        let b = Identity::human(&mut rng);
        assert_ne!(a, b);
    }

    #[test]
    fn both_genders_turn_up_over_enough_rolls() {
        let mut rng = SmallRng::seed_from_u64(3);
        let mut saw = (false, false);
        for _ in 0..1000 {
            match Identity::human(&mut rng).gender() {
                Gender::Male => saw.0 = true,
                Gender::Female => saw.1 = true,
            }
        }
        assert_eq!(saw, (true, true));
    }
}
