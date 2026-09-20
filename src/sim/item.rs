//! Things a unit can hold: [`ItemKind`].
//!
//! An item says what it is made of — how nourishing, how much water, how heavy
//! and how bulky — and not what that does to whoever consumes it or carries it.
//! That is somebody else's business: the processes in [`super::biology`] read
//! the nourishment when they hear it was eaten, so a drink filling a bladder is
//! the bladder's doing and not the water's, and [`super::inventory`] reads the
//! mass and the volume to decide what will still fit.
//!
//! # Kinds, not things
//!
//! An `ItemKind` is fieldless and `Copy`: two portions of food are the same
//! item in every way the simulation can tell them apart. That is what lets an
//! [`Inventory`](super::inventory::Inventory) be a count per kind rather than a
//! list of objects — no allocation, no ids, and "three food" is the whole
//! truth. An item that needs state of its own (a half-eaten meal, a labelled
//! crate) is a change *here* first, and the inventory follows it.

use super::clock::{watched, MINUTE};

/// How nourishing one meal is: the hunger it takes away.
pub const MEAL: f32 = 60.0;

/// How much water one drink is: the thirst it takes away.
pub const DRINK: f32 = 60.0;

/// What a portion of food weighs, in kilograms.
pub const MEAL_MASS: f32 = 0.5;

/// ...and the space it takes up, in litres.
///
/// Bulky for its weight, the way a boxed meal is mostly box. Paired with
/// [`DRINK_VOLUME`] below this is what makes a unit's two limits both mean
/// something: food runs out of *space* first and water runs out of *strength*
/// first, so neither limit is decoration. See
/// [`Capacity::HUMAN`](super::inventory::Capacity::HUMAN).
pub const MEAL_VOLUME: f32 = 1.0;

/// What a glass of water weighs, in kilograms.
pub const DRINK_MASS: f32 = 0.5;

/// ...and the space it takes up, in litres. Water is dense: it weighs what it
/// measures, which is exactly why it is the mass limit that stops you carrying
/// more of it.
pub const DRINK_VOLUME: f32 = 0.5;

/// Time spent eating food: a quarter of an hour at the table, seven and a half
/// seconds of watching it.
///
/// Written in world minutes and converted, like every duration a body is
/// *watched* standing through — the action's own clock is the watched one
/// (see [`crate::sim::clock`]), and this is what that quarter hour comes to on
/// it.
pub const CHEW_SECONDS: f32 = watched(15.0 * MINUTE);

/// Time spent drinking water: a couple of minutes, a fraction of a meal.
pub const SIP_SECONDS: f32 = watched(2.0 * MINUTE);

/// What is being held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Food,
    Water,
}

impl ItemKind {
    /// Every kind there is, in declaration order.
    ///
    /// An [`Inventory`](super::inventory::Inventory) is a count per kind
    /// indexed by this, and its debug view walks it — so the order here is the
    /// order a unit's items are listed in, and iterating it can never be a hash
    /// order.
    pub const ALL: [ItemKind; ItemKind::COUNT] = [ItemKind::Food, ItemKind::Water];

    /// How many kinds there are. A literal, because it sizes an array.
    /// `every_kind_is_in_all` fails if the two stop agreeing.
    pub const COUNT: usize = 2;

    /// How it reads in a debug view.
    pub const fn name(self) -> &'static str {
        match self {
            ItemKind::Food => "food",
            ItemKind::Water => "water",
        }
    }

    /// How long using this up takes.
    ///
    /// On the item, rather than in whichever goal consumes it: a goal that
    /// finds its hand full of something it did not fetch has to finish it
    /// first, and should not have to know what it is to know how long that
    /// takes.
    pub const fn consume_seconds(self) -> f32 {
        match self {
            ItemKind::Food => CHEW_SECONDS,
            ItemKind::Water => SIP_SECONDS,
        }
    }

    /// How nourishing it is, on the hunger scale.
    pub const fn nutrition(self) -> f32 {
        match self {
            ItemKind::Food => MEAL,
            ItemKind::Water => 0.0,
        }
    }

    /// How much water is in it, on the thirst scale.
    pub const fn hydration(self) -> f32 {
        match self {
            ItemKind::Food => 0.0,
            ItemKind::Water => DRINK,
        }
    }

    /// What it weighs, in kilograms.
    ///
    /// Counted against a unit's mass limit wherever it is —  in a hand or
    /// stowed away — because weight is carried either way.
    pub const fn mass(self) -> f32 {
        match self {
            ItemKind::Food => MEAL_MASS,
            ItemKind::Water => DRINK_MASS,
        }
    }

    /// How much room it takes up, in litres.
    ///
    /// Counted only against what is **stowed**: space is about packing things
    /// away, and something held in a hand is not packed. See
    /// [`Inventory`](super::inventory::Inventory).
    pub const fn volume(self) -> f32 {
        match self {
            ItemKind::Food => MEAL_VOLUME,
            ItemKind::Water => DRINK_VOLUME,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_in_all() {
        assert_eq!(ItemKind::ALL.len(), ItemKind::COUNT);
        for (index, kind) in ItemKind::ALL.iter().enumerate() {
            assert_eq!(*kind as usize, index, "{} is filed under the wrong index", kind.name());
        }
    }

    #[test]
    fn everything_that_can_be_carried_weighs_and_measures_something() {
        // An item with no mass or no volume would be free to carry, and a
        // limit nothing counts against is not a limit.
        for kind in ItemKind::ALL {
            assert!(kind.mass() > 0.0, "{} weighs nothing", kind.name());
            assert!(kind.volume() > 0.0, "{} takes up no room", kind.name());
        }
    }
}
