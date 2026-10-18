//! What a human is wearing. Mutable state, independent of personal identity.
//! Names are stable save keys; texture paths belong to `characters::human`.

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
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|outfit| outfit.name() == name)
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
    }
}

