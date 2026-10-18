//! What a human is wearing. Mutable state, independent of personal identity.
//! Names are stable save keys; texture paths belong to `characters::human`.
//!
//! Two kinds of outfit: the **wardrobe** ([`Clothing::ALL`]) a crowd is
//! dressed from and a player picks from, and the **work uniforms**
//! ([`Clothing::UNIFORMS`]), one per job (`sim::job::JobKind::uniform`), which
//! nobody owns: a worker is in its job's uniform while its shift is due and
//! back in its own clothes after (`GameEntity::worn`).

use rand::{Rng, RngExt, SeedableRng};
use rand::rngs::SmallRng;
use rand::seq::IndexedRandom;

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
    /// A white shirt and a navy tie: the office worker's uniform.
    UniformOfficeWorker,
    /// Hi-vis overalls: the scrap collector's.
    UniformScrapCollector,
    /// A navy tunic with brass buttons and a badge: the guard's.
    UniformGuard,
    /// A black cassock with a white collar and a gold cross: the
    /// undertaker's.
    UniformUndertaker,
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

    /// The work uniforms: not in [`Clothing::ALL`], so never handed out at
    /// random and not on the clothing menu — a job puts one on.
    pub const UNIFORMS: [Self; 4] =
        [Self::UniformOfficeWorker, Self::UniformScrapCollector, Self::UniformGuard, Self::UniformUndertaker];

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
            Self::UniformOfficeWorker => "uniform office worker",
            Self::UniformScrapCollector => "uniform scrap collector",
            Self::UniformGuard => "uniform guard",
            Self::UniformUndertaker => "uniform undertaker",
        }
    }

    /// Any outfit by its name, the wardrobe's or a uniform.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().chain(Self::UNIFORMS).find(|outfit| outfit.name() == name)
    }

    pub const fn is_uniform(self) -> bool {
        matches!(
            self,
            Self::UniformOfficeWorker | Self::UniformScrapCollector | Self::UniformGuard | Self::UniformUndertaker
        )
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
        for uniform in Clothing::UNIFORMS {
            assert!(uniform.is_uniform());
            assert_eq!(Clothing::from_name(uniform.name()), Some(uniform));
            assert!(!Clothing::ALL.contains(&uniform), "nobody is dressed in a uniform at random");
        }
        assert!(!Clothing::ALL.iter().any(|outfit| outfit.is_uniform()));
    }
}

