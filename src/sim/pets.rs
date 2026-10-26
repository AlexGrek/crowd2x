//! Pet dogs: what a dog's belly is, who is bonded to whom, and meals served.
//!
//! A pet is a [`Dog`](super::kinds::Dog) with an owner — one person, who has
//! the dog as its companion (`Memory::companion`, on both sides). The dog
//! stays close to its owner, sleeps nearby while its owner is asleep,
//! goes to the toilet on the street, and **it
//! cannot feed itself**: its owner buys dog food from a fridge and gives it
//! to it (`brain::goals::DogCareGoal`).
//!
//! # The gut ([`Gut`])
//!
//! A dog has no [`Biology`](super::biology::Biology): two numbers and a
//! third kept between them are all the body it has.
//!
//! * `hunger` rises with the world's hours ([`HOURS_TO_STARVING`]) and a meal
//!   takes [`MEAL`] off it. Past [`HUNGRY`] its owner is moved to feed it.
//! * A meal is **digested** over [`DIGEST_HOURS`] into the `bowel`, which at
//!   [`MUST_GO`] the dog empties on the street (`brain::goals::DefecateGoal`).
//!
//! Nothing happens to a dog left hungry yet: it stays close to its owner
//! and begs. That is for later.
//!
//! # Who knows whom, with no scan of the crowd ([`Bonds`])
//!
//! A dog follows an owner it can not read the entity of, and an owner looks
//! to a dog it can not read either — **react writes only to itself**. So the
//! think step, which touches every unit anyway, has each bonded one say who it
//! is bonded to, where it stands and, for a dog, how hungry and how full it
//! is ([`GameEntity::bond`](super::GameEntity::bond)); [`Bonds::rebuild`]
//! sorts them, and react binary-searches the few. Rebuilt every tick, never
//! saved.
//!
//! # Meals served ([`Feedings`])
//!
//! The dog is fed by somebody else, which is a change to a body that is not
//! the one asking: `Effect::Feed` files a [`Feeding`] and the dog **feels it in
//! its own react** next tick, as a bite is felt (`sim::wounds`). Held between
//! ticks, so saved. The dog's bond already counts a meal waiting to be felt,
//! or for a tick its owner would take it for still hungry and set off for
//! another dinner.

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::clock::HOUR;
use super::uid::Uid;

/// World hours from a full belly to starving, with nothing eaten.
pub const HOURS_TO_STARVING: f32 = 16.0;

/// Hunger a dog food takes away.
pub const MEAL: f32 = 60.0;

/// Hunger at which its owner sees to feeding it: about eight hours after a
/// full meal, so three meals a day.
pub const HUNGRY: f32 = 50.0;

/// World hours a meal takes to pass through.
pub const DIGEST_HOURS: f32 = 3.0;

/// Bowel at which it must go — two and a half hours after a meal.
pub const MUST_GO: f32 = 50.0;

/// How near, in cells either way, an owner must be to feed its dog, and the
/// dog to be fed.
pub const FEED_REACH: i32 = 2;

/// In a district, one resident in this many has a pet dog, chosen by the
/// random half of its id (`GameState::adopt_pets`).
pub const ONE_IN: u64 = 8;

/// A dog's belly: how hungry it is, what it has eaten and not yet passed, and
/// how full its bowel is. 0 to 100 each (the last, what is still to be
/// digested, to 100 of a meal's worth).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Gut {
    hunger: f32,
    digesting: f32,
    bowel: f32,
}

impl Gut {
    /// A gut this hungry, which has eaten nothing.
    pub fn hungry_to(hunger: f32) -> Gut {
        Gut { hunger: hunger.clamp(0.0, 100.0), ..Gut::default() }
    }

    pub fn hunger(&self) -> f32 {
        self.hunger
    }

    pub fn bowel(&self) -> f32 {
        self.bowel
    }

    pub fn digesting(&self) -> f32 {
        self.digesting
    }

    /// `dt` **world** seconds go by: hungrier, and some of the last meal
    /// reaches the bowel.
    pub fn advance(&mut self, dt: f32) {
        self.hunger = (self.hunger + 100.0 / (HOURS_TO_STARVING * HOUR) * dt).min(100.0);
        let passed = self.digesting.min(MEAL / (DIGEST_HOURS * HOUR) * dt);
        self.digesting -= passed;
        self.bowel = (self.bowel + passed).min(100.0);
    }

    /// A dog food eaten.
    pub fn fed(&mut self) {
        self.hunger = (self.hunger - MEAL).max(0.0);
        self.digesting = (self.digesting + MEAL).min(100.0);
    }

    /// The bowel emptied.
    pub fn relieved(&mut self) {
        self.bowel = 0.0;
    }

    pub fn is_hungry(&self) -> bool {
        self.hunger >= HUNGRY
    }

    pub fn must_go(&self) -> bool {
        self.bowel >= MUST_GO
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Gut {
        let Gut { hunger, digesting, bowel } = *self;
        pb::Gut { hunger, digesting, bowel }
    }

    pub(crate) fn restore(saved: &pb::Gut) -> Result<Gut, Invalid> {
        let fine = |value: f32| value.is_finite() && (0.0..=100.0).contains(&value);
        if !(fine(saved.hunger) && fine(saved.digesting) && fine(saved.bowel)) {
            return invalid(format!("a gut of {}, {} and {}", saved.hunger, saved.digesting, saved.bowel));
        }
        Ok(Gut { hunger: saved.hunger, digesting: saved.digesting, bowel: saved.bowel })
    }
}

/// One side of a bond, as the rest of the crowd sees it this tick.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bond {
    /// Whose it is: an owner's or a dog's.
    pub who: Uid,
    /// The other: its dog, or its owner.
    pub with: Uid,
    /// The cell it stood in as the last tick ended.
    pub cell: Point,
    /// A dog's hunger, with a meal waiting to be felt counted; 0 for an owner.
    pub hunger: f32,
    /// A dog's bowel; 0 for an owner.
    pub bowel: f32,
    /// Actually asleep, rather than walking to a bed. Rebuilt every tick.
    pub asleep: bool,
}

/// **Who is bonded to whom this tick** ([`Bond`]), sorted by who. A dozen or
/// so in a district, so a binary search: the owner's dog and the dog's owner
/// are both one `get` away.
#[derive(Clone, Debug, Default)]
pub struct Bonds {
    sorted: Vec<Bond>,
}

impl Bonds {
    /// Everybody in `slots` who is bonded, ready to be asked.
    pub fn rebuild(&mut self, slots: &[Option<Bond>]) {
        self.sorted.clear();
        self.sorted.extend(slots.iter().flatten().copied());
        self.sorted.sort_unstable_by_key(|bond| bond.who);
    }

    /// `who`'s bond, if it has one.
    pub fn get(&self, who: Uid) -> Option<&Bond> {
        self.sorted.binary_search_by_key(&who, |bond| bond.who).ok().map(|at| &self.sorted[at])
    }

    /// The bond of whoever `who` is bonded to — its dog's, for an owner, and
    /// its owner's, for a dog — if that one is bonded back.
    pub fn partner(&self, who: Uid) -> Option<&Bond> {
        self.get(who).and_then(|bond| self.get(bond.with)).filter(|partner| partner.with == who)
    }

    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sorted.is_empty()
    }

    /// Say `who` has `bond`, or none: for a world of one
    /// (`testing::World`), which has no think step over a crowd.
    pub fn set(&mut self, who: Uid, bond: Option<Bond>) {
        self.sorted.retain(|other| other.who != who);
        self.sorted.extend(bond);
        self.sorted.sort_unstable_by_key(|bond| bond.who);
    }
}

/// A meal served: `by` gave `dog` a dog food.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Feeding {
    pub dog: Uid,
    pub by: Uid,
}

/// The meals served this tick, waiting for the dogs to feel them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feedings {
    served: Vec<Feeding>,
}

impl Feedings {
    /// Forget the meals the last tick served, felt since.
    pub fn begin_tick(&mut self) {
        self.served.clear();
    }

    pub fn serve(&mut self, feeding: Feeding) {
        self.served.push(feeding);
    }

    /// The meals served to `dog`, oldest first. None nearly always, so a walk
    /// of the list.
    pub fn to(&self, dog: Uid) -> impl Iterator<Item = &Feeding> + '_ {
        self.served.iter().filter(move |feeding| feeding.dog == dog)
    }

    pub fn is_empty(&self) -> bool {
        self.served.is_empty()
    }

    // --- saving ---

    pub(crate) fn save(&self) -> Vec<pb::Feeding> {
        let Feedings { served } = self;
        served.iter().map(|&Feeding { dog, by }| pb::Feeding { dog: dog.raw(), by: by.raw() }).collect()
    }

    pub(crate) fn restore(saved: &[pb::Feeding]) -> Result<Feedings, Invalid> {
        let mut feedings = Feedings::default();
        for feeding in saved {
            let (Some(dog), Some(by)) = (Uid::from_raw(feeding.dog), Uid::from_raw(feeding.by)) else {
                return invalid("a meal served by or to nobody");
            };
            feedings.serve(Feeding { dog, by });
        }
        Ok(feedings)
    }
}

/// A pet and its owner for a test's world (`testing::World`), which has no
/// think step to gather their bonds: the owner is whoever
/// `testing::needy_human` makes, and a test says where it stands.
#[cfg(test)]
pub(crate) mod rig {
    use super::*;
    use crate::sim::kinds::{Dog, Facing};
    use crate::sim::testing::World;
    use crate::sim::uid::EntityType;
    use crate::sim::GameEntity;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    /// `testing::needy_human`'s uid.
    pub fn owner_uid() -> Uid {
        Uid::new(EntityType::Human, 77)
    }

    pub fn dog_uid() -> Uid {
        Uid::new(EntityType::Dog, 7)
    }

    /// The owner's dog, at `at`, `hunger` hungry and with nothing in its
    /// bowel.
    pub fn dog(at: Point, hunger: f32) -> Dog {
        let mut dog = Dog::new(dog_uid(), at, Facing::Left, &mut SmallRng::seed_from_u64(1));
        assert!(dog.set_companion(owner_uid()));
        *dog.gut_mut() = Gut::hungry_to(hunger);
        dog
    }

    /// The owner's dog with a meal passed through it: it must go.
    pub fn dog_that_must_go(at: Point) -> Dog {
        let mut dog = dog(at, 0.0);
        let gut = dog.gut_mut();
        gut.fed();
        gut.advance(DIGEST_HOURS * HOUR);
        assert!(gut.must_go());
        dog
    }

    /// Say the owner stands at `at`, as the think step would for a crowd.
    pub fn owner_stands(world: &mut World, at: Point) {
        world.bonds.set(
            owner_uid(),
            Some(Bond { who: owner_uid(), with: dog_uid(), cell: at, hunger: 0.0, bowel: 0.0, asleep: false }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::uid::EntityType;

    #[test]
    fn a_dog_gets_hungrier_with_the_hours_and_never_past_starving() {
        let mut gut = Gut::hungry_to(0.0);
        gut.advance(8.0 * HOUR);
        assert!((gut.hunger() - 50.0).abs() < 0.01, "{}", gut.hunger());
        assert!(gut.is_hungry());
        gut.advance(100.0 * HOUR);
        assert_eq!(gut.hunger(), 100.0);
    }

    #[test]
    fn a_meal_takes_hunger_away_and_reaches_the_bowel_over_hours() {
        let mut gut = Gut::hungry_to(70.0);
        gut.fed();
        assert_eq!(gut.hunger(), 10.0);
        assert!(!gut.must_go(), "not at once");
        gut.advance(2.0 * HOUR);
        assert!(!gut.must_go(), "bowel {}", gut.bowel());
        gut.advance(1.0 * HOUR);
        assert!(gut.must_go(), "bowel {}", gut.bowel());
        assert!((gut.bowel() - 60.0).abs() < 0.5, "the whole meal passed: {}", gut.bowel());
        assert_eq!(gut.digesting(), 0.0);
        gut.relieved();
        assert_eq!(gut.bowel(), 0.0);
        assert!(!gut.must_go());
    }

    #[test]
    fn a_meal_never_takes_a_dog_below_full() {
        let mut gut = Gut::hungry_to(20.0);
        gut.fed();
        assert_eq!(gut.hunger(), 0.0);
    }

    #[test]
    fn a_gut_is_saved_and_a_bad_one_refused() {
        let mut gut = Gut::hungry_to(33.0);
        gut.fed();
        gut.advance(HOUR);
        assert_eq!(Gut::restore(&gut.save()).unwrap(), gut);
        assert!(Gut::restore(&pb::Gut { hunger: f32::NAN, digesting: 0.0, bowel: 0.0 }).is_err());
        assert!(Gut::restore(&pb::Gut { hunger: 0.0, digesting: 0.0, bowel: 101.0 }).is_err());
    }

    #[test]
    fn bonds_are_found_by_who_and_meals_by_dog_until_the_next_tick() {
        let owner = Uid::new(EntityType::Human, 5);
        let dog = Uid::new(EntityType::Dog, 6);
        let other = Uid::new(EntityType::Dog, 7);
        let mut bonds = Bonds::default();
        bonds.rebuild(&[
            Some(Bond { who: owner, with: dog, cell: Point::new(1, 1), hunger: 0.0, bowel: 0.0, asleep: false }),
            None,
            Some(Bond { who: dog, with: owner, cell: Point::new(2, 1), hunger: 55.0, bowel: 10.0, asleep: false }),
        ]);
        assert_eq!(bonds.get(dog).map(|bond| bond.with), Some(owner));
        assert_eq!(bonds.get(owner).map(|bond| bond.cell), Some(Point::new(1, 1)));
        assert!(bonds.get(other).is_none());
        bonds.set(dog, None);
        assert!(bonds.get(dog).is_none() && bonds.len() == 1);

        let mut feedings = Feedings::default();
        feedings.serve(Feeding { dog, by: owner });
        assert_eq!(feedings.to(dog).count(), 1);
        assert_eq!(feedings.to(other).count(), 0);
        assert_eq!(Feedings::restore(&feedings.save()).unwrap(), feedings);
        feedings.begin_tick();
        assert!(feedings.is_empty());
    }
}
