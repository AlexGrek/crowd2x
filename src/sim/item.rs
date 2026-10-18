//! Things a unit can hold: [`ItemKind`].
//!
//! An item says what it is made of — how nourishing, how much water, how tasty, how heavy
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

/// How much of a treat one meal is: the satisfaction it gives, the first time.
///
/// Not how filling it is — that is [`MEAL`], and it is the same every time.
/// What the treat is worth *this* time is for the body to say, by how recently
/// it had the same thing (`biology::Recollection`).
pub const TASTY: f32 = 15.0;

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

/// What a piece of scrap metal salvaged from a heap in the wasteland weighs,
/// in kilograms: a bent panel, a length of pipe, a fistful of bolts.
///
/// Heavy, so the mass limit is the one it meets: a person carries three
/// pieces and no more — exactly the most a heap holds
/// ([`MAX_SCRAP`](crate::map::wasteland::MAX_SCRAP)) — and a fourth waits for
/// a hand that is free.
pub const SCRAP_MASS: f32 = 3.0;

/// ...and the space it takes up, in litres.
pub const SCRAP_VOLUME: f32 = 2.0;

/// How long a piece of scrap metal in a hand takes to be got rid of by a
/// goal that wants the hand: a moment. There is nothing to consume in it,
/// and the body gets nothing from it; only a QA script's `hold` ever puts
/// one in a hand, salvage stowing it straight away.
pub const DROP_SECONDS: f32 = watched(MINUTE);

/// What a body carried to the crematorium weighs, in kilograms. Far over
/// what anybody can stow, so it is only ever **in a hand** — which is never
/// refused (`Inventory::set_hand`) — and nothing else can be taken up while
/// it is carried.
pub const BODY_MASS: f32 = 70.0;

/// ...and the space it takes up, in litres.
pub const BODY_VOLUME: f32 = 70.0;

/// How long a body in a hand takes a goal that wants the hand to be rid of:
/// laid down, a moment, and gone — a corpse put down is not picked up again.
pub const LAY_DOWN_SECONDS: f32 = watched(MINUTE);

/// What is being held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Food,
    Water,
    /// A piece of scrap metal, salvaged from a heap in the wasteland
    /// (`brain::tasks::Salvage`). Nothing a body can use: it is carried.
    Scrap,
    /// The dead, taken up by an undertaker (`brain::tasks::TakeBody`) and
    /// carried to a crematorium. Nothing a body can use.
    Body,
}

impl ItemKind {
    /// Every kind there is, in declaration order.
    ///
    /// An [`Inventory`](super::inventory::Inventory) is a count per kind
    /// indexed by this, and its debug view walks it — so the order here is the
    /// order a unit's items are listed in, and iterating it can never be a hash
    /// order.
    pub const ALL: [ItemKind; ItemKind::COUNT] = [ItemKind::Food, ItemKind::Water, ItemKind::Scrap, ItemKind::Body];

    /// How many kinds there are. A literal, because it sizes an array.
    /// `every_kind_is_in_all` fails if the two stop agreeing.
    pub const COUNT: usize = 4;

    /// How it reads in a debug view.
    pub const fn name(self) -> &'static str {
        match self {
            ItemKind::Food => "food",
            ItemKind::Water => "water",
            ItemKind::Scrap => "scrap metal",
            ItemKind::Body => "body",
        }
    }

    /// The kind called `name` ([`ItemKind::name`]) — how a save names one.
    pub fn from_name(name: &str) -> Option<ItemKind> {
        ItemKind::ALL.into_iter().find(|item| item.name() == name)
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
            ItemKind::Scrap => DROP_SECONDS,
            ItemKind::Body => LAY_DOWN_SECONDS,
        }
    }

    /// How nourishing it is, on the hunger scale.
    pub const fn nutrition(self) -> f32 {
        match self {
            ItemKind::Food => MEAL,
            ItemKind::Water | ItemKind::Scrap | ItemKind::Body => 0.0,
        }
    }

    /// How much water is in it, on the thirst scale.
    pub const fn hydration(self) -> f32 {
        match self {
            ItemKind::Food | ItemKind::Scrap | ItemKind::Body => 0.0,
            ItemKind::Water => DRINK,
        }
    }

    /// How much of a treat it is, on the satisfaction scale. Zero for
    /// something with no flavour to speak of — and something with no flavour
    /// is nothing to tire of either.
    pub const fn taste(self) -> f32 {
        match self {
            ItemKind::Food => TASTY,
            ItemKind::Water | ItemKind::Scrap | ItemKind::Body => 0.0,
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
            ItemKind::Scrap => SCRAP_MASS,
            ItemKind::Body => BODY_MASS,
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
            ItemKind::Scrap => SCRAP_VOLUME,
            ItemKind::Body => BODY_VOLUME,
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

    /// Scrap is carried, not consumed: nothing in it for a body.
    #[test]
    fn scrap_metal_is_nothing_a_body_can_use() {
        let scrap = ItemKind::Scrap;
        assert_eq!((scrap.nutrition(), scrap.hydration(), scrap.taste()), (0.0, 0.0, 0.0));
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
