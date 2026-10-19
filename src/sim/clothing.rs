//! What a human is wearing. Mutable state, independent of personal identity.
//! Names are stable save keys; texture paths belong to `characters::human`.
//!
//! Two kinds of outfit: the **wardrobe** ([`Clothing::ALL`]) a crowd is
//! dressed from and a player picks from, and the **work uniforms**
//! ([`Clothing::Uniform`], [`Clothing::uniforms`]), one per job, which nobody
//! owns: a worker is in its job's uniform while its shift is due and back in
//! its own clothes after (`GameEntity::worn`). A uniform is named in its job's
//! row (`sim::job::JobDef::uniform`), so a new job brings its own and nothing
//! here changes.

use rand::{Rng, RngExt, SeedableRng};
use rand::rngs::SmallRng;
use rand::seq::IndexedRandom;

use super::job::JobKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clothing {
    None,
    SwimsuitBlue,
    SwimsuitPink,
    SundressRed,
    SundressYellow,
    SundressGreen,
    SundressWhite,
    SundressTeal,
    SundressOrange,
    SundressPurple,
    GownRed,
    GownBlack,
    GownNavy,
    GownPurple,
    GownWhite,
    GownGreen,
    ShortsWhiteJeans,
    ShortsRedKhaki,
    ShortsYellowJeans,
    ShortsTealCharcoal,
    TrousersWhiteCharcoal,
    TrousersBlackJeans,
    TrousersGreenKhaki,
    TrousersNavyBrown,
    TrousersOrangeJeans,
    /// A job's work uniform: the office worker's white shirt and navy tie,
    /// the scrap collector's hi-vis, the guard's tunic, the undertaker's
    /// cassock, the steelworker's apron, the courier's jacket.
    Uniform(JobKind),
}

impl Clothing {
    pub const ALL: [Self; 25] = [
        Self::None,
        Self::SwimsuitBlue,
        Self::SwimsuitPink,
        Self::SundressRed,
        Self::SundressYellow,
        Self::SundressGreen,
        Self::SundressWhite,
        Self::SundressTeal,
        Self::SundressOrange,
        Self::SundressPurple,
        Self::GownRed,
        Self::GownBlack,
        Self::GownNavy,
        Self::GownPurple,
        Self::GownWhite,
        Self::GownGreen,
        Self::ShortsWhiteJeans,
        Self::ShortsRedKhaki,
        Self::ShortsYellowJeans,
        Self::ShortsTealCharcoal,
        Self::TrousersWhiteCharcoal,
        Self::TrousersBlackJeans,
        Self::TrousersGreenKhaki,
        Self::TrousersNavyBrown,
        Self::TrousersOrangeJeans,
    ];

    /// The work uniforms, one per job: not in [`Clothing::ALL`], so never
    /// handed out at random and not on the clothing menu — a job puts one on.
    pub fn uniforms() -> impl Iterator<Item = Self> {
        JobKind::ALL.into_iter().map(Self::Uniform)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SwimsuitBlue => "swimsuit blue",
            Self::SwimsuitPink => "swimsuit pink",
            Self::SundressRed => "sundress red",
            Self::SundressYellow => "sundress yellow",
            Self::SundressGreen => "sundress green",
            Self::SundressWhite => "sundress white",
            Self::SundressTeal => "sundress teal",
            Self::SundressOrange => "sundress orange",
            Self::SundressPurple => "sundress purple",
            Self::GownRed => "gown red",
            Self::GownBlack => "gown black",
            Self::GownNavy => "gown navy",
            Self::GownPurple => "gown purple",
            Self::GownWhite => "gown white",
            Self::GownGreen => "gown green",
            Self::ShortsWhiteJeans => "shorts white jeans",
            Self::ShortsRedKhaki => "shorts red khaki",
            Self::ShortsYellowJeans => "shorts yellow jeans",
            Self::ShortsTealCharcoal => "shorts teal charcoal",
            Self::TrousersWhiteCharcoal => "trousers white charcoal",
            Self::TrousersBlackJeans => "trousers black jeans",
            Self::TrousersGreenKhaki => "trousers green khaki",
            Self::TrousersNavyBrown => "trousers navy brown",
            Self::TrousersOrangeJeans => "trousers orange jeans",
            Self::Uniform(job) => job.def().uniform,
        }
    }

    /// Any outfit by its name, the wardrobe's or a uniform.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().chain(Self::uniforms()).find(|outfit| outfit.name() == name)
    }

    pub const fn is_uniform(self) -> bool {
        matches!(self, Self::Uniform(_))
    }

    /// The original seeded outfit, also used when loading saves made before
    /// clothing was mutable. Uses no world RNG, so other spawn rolls stay fixed.
    pub fn initial(seed: u64) -> Self {
        Self::random(&mut SmallRng::seed_from_u64(seed))
    }

    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        if rng.random_range(0..100) < 85 {
            *Self::ALL[1..].choose(rng).expect("outfits exist")
        } else {
            Self::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outfit_has_a_unique_stable_name() {
        for (index, outfit) in Clothing::ALL.into_iter().enumerate() {
            assert_eq!(Clothing::from_name(outfit.name()), Some(outfit));
            assert!(!Clothing::ALL[..index].iter().any(|other| other.name() == outfit.name()));
        }
        assert_eq!(Clothing::from_name("unknown outfit"), None);
        for uniform in Clothing::uniforms() {
            assert!(uniform.is_uniform());
            assert!(uniform.name().starts_with("uniform "), "{}", uniform.name());
            assert_eq!(Clothing::uniforms().filter(|other| other.name() == uniform.name()).count(), 1);
            assert_eq!(Clothing::from_name(uniform.name()), Some(uniform));
            assert!(!Clothing::ALL.contains(&uniform), "nobody is dressed in a uniform at random");
        }
        assert!(!Clothing::ALL.iter().any(|outfit| outfit.is_uniform()));
    }
}

