//! A person's lasting aptitudes, separate from their changing biological stats.
//!
//! Rolled once at spawn from independent normal distributions (mean 50,
//! standard deviation 15), rounded to integers and truncated to 0..=100.
//! Time alone does not change them; an explicit simulation command can.

use rand::RngExt;
use rand::rngs::SmallRng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Talent {
    Arts,
    Engineering,
    Communication,
    Management,
    Athleticism,
    Military,
    Beauty,
    Responsibility,
    Precision,
}

impl Talent {
    pub const ALL: [Talent; 9] = [
        Self::Arts,
        Self::Engineering,
        Self::Communication,
        Self::Management,
        Self::Athleticism,
        Self::Military,
        Self::Beauty,
        Self::Responsibility,
        Self::Precision,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Arts => "arts",
            Self::Engineering => "engineering",
            Self::Communication => "communication",
            Self::Management => "management",
            Self::Athleticism => "athleticism",
            Self::Military => "military",
            Self::Beauty => "beauty",
            Self::Responsibility => "responsibility",
            Self::Precision => "precision",
        }
    }
}

/// Nine bounded integers, stored inline on a human. Mutation keeps the bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Talents {
    values: [u8; Talent::ALL.len()],
}

impl Talents {
    pub fn random(rng: &mut SmallRng) -> Self {
        Self {
            values: std::array::from_fn(|_| normal_score(rng)),
        }
    }

    pub fn get(&self, talent: Talent) -> u8 {
        self.values[talent as usize]
    }

    /// Explicit changes are bounded too, including negative inputs.
    pub fn set(&mut self, talent: Talent, value: i32) {
        self.values[talent as usize] = value.clamp(0, 100) as u8;
    }
}

fn normal_score(rng: &mut SmallRng) -> u8 {
    loop {
        // Box–Muller: (0, 1] keeps ln(0) out of the transform. Reject tails
        // instead of clamping, which would pile them up at the endpoints.
        let radius = (-2.0 * (1.0 - rng.random::<f64>()).ln()).sqrt();
        let angle = std::f64::consts::TAU * rng.random::<f64>();
        let score = (50.0 + 15.0 * radius * angle.cos()).round();
        if (0.0..=100.0).contains(&score) {
            return score as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn the_same_seed_generates_the_same_talents_and_people_differ() {
        let mut a = SmallRng::seed_from_u64(42);
        let mut b = SmallRng::seed_from_u64(42);
        let first = Talents::random(&mut a);
        assert_eq!(first, Talents::random(&mut b));
        let second = Talents::random(&mut a);
        assert_eq!(second, Talents::random(&mut b));
        assert_ne!(first, second);
    }

    #[test]
    fn every_talent_has_a_bounded_bell_shaped_distribution() {
        let mut rng = SmallRng::seed_from_u64(7);
        let people: Vec<_> = (0..20_000).map(|_| Talents::random(&mut rng)).collect();
        for talent in Talent::ALL {
            let scores: Vec<_> = people.iter().map(|p| f64::from(p.get(talent))).collect();
            assert!(scores.iter().all(|v| (0.0..=100.0).contains(v)));
            let mean = scores.iter().sum::<f64>() / scores.len() as f64;
            let variance =
                scores.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / scores.len() as f64;
            assert!((mean - 50.0).abs() < 0.5, "{talent:?}: mean {mean}");
            assert!(
                (variance.sqrt() - 15.0).abs() < 0.5,
                "{talent:?}: variance {variance}"
            );
            let central = scores.iter().filter(|v| (35.0..=65.0).contains(*v)).count();
            let fraction = central as f64 / scores.len() as f64;
            assert!(
                (0.67..0.72).contains(&fraction),
                "{talent:?}: central fraction {fraction}"
            );
            assert!(scores.iter().any(|v| *v < 10.0));
            assert!(scores.iter().any(|v| *v > 90.0));
        }
        // Talents are separate draws, rather than one score copied nine times.
        let mean_product = people
            .iter()
            .map(|p| {
                (f64::from(p.get(Talent::Arts)) - 50.0)
                    * (f64::from(p.get(Talent::Engineering)) - 50.0)
            })
            .sum::<f64>()
            / people.len() as f64;
        assert!(
            mean_product.abs() < 10.0,
            "correlated talents: {mean_product}"
        );
    }

    #[test]
    fn explicit_changes_keep_the_range_and_leave_other_talents_alone() {
        let mut talents = Talents::random(&mut SmallRng::seed_from_u64(1));
        let before = talents;
        talents.set(Talent::Precision, 101);
        assert_eq!(talents.get(Talent::Precision), 100);
        talents.set(Talent::Precision, -1);
        assert_eq!(talents.get(Talent::Precision), 0);
        talents.set(Talent::Precision, 73);
        assert_eq!(talents.get(Talent::Precision), 73);
        for talent in Talent::ALL {
            if talent != Talent::Precision {
                assert_eq!(talents.get(talent), before.get(talent));
            }
        }
    }
}
