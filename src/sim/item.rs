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

use super::catalogue::catalogue;
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

/// What an ingot of steel weighs, in kilograms: the steel factory's
/// furnace makes one of [`SCRAP_PER_STEEL`] pieces of scrap metal. Heavier
/// than a piece of scrap, so a courier carries two at a time.
pub const STEEL_MASS: f32 = 5.0;

/// ...and the space it takes up, in litres: dense.
pub const STEEL_VOLUME: f32 = 1.0;

/// How many pieces of scrap metal a furnace smelts into one ingot of steel.
pub const SCRAP_PER_STEEL: u8 = 2;

/// What a sheaf of wheat harvested from a crop weighs, in kilograms
/// (`sim::fields`): light, and bulky, so a pack takes twelve — four crops'
/// worth at their best.
pub const WHEAT_MASS: f32 = 0.5;

/// ...and the space it takes up, in litres.
pub const WHEAT_VOLUME: f32 = 1.0;

/// Everything about one kind of item: its row in [`ItemKind`]'s catalogue.
pub struct ItemDef {
    /// How it reads in a debug view, and how a save names it.
    pub name: &'static str,
    /// How long using it up takes ([`ItemKind::consume_seconds`]).
    pub consume_seconds: f32,
    /// How nourishing it is, on the hunger scale.
    pub nutrition: f32,
    /// How much water is in it, on the thirst scale.
    pub hydration: f32,
    /// How much of a treat it is, on the satisfaction scale.
    pub taste: f32,
    /// What it weighs, in kilograms.
    pub mass: f32,
    /// How much room it takes up, in litres.
    pub volume: f32,
    /// How it is drawn in a hand (`characters::item`): one emoji, a single
    /// codepoint, for the reason [`GoalId::emoji`](super::brain::GoalId::emoji)
    /// is.
    pub emoji: &'static str,
}

/// A thing nobody eats or drinks: carried, and dropped in a moment if a
/// hand is wanted for something else.
const CARRIED: ItemDef = ItemDef {
    name: "",
    consume_seconds: DROP_SECONDS,
    nutrition: 0.0,
    hydration: 0.0,
    taste: 0.0,
    mass: 0.0,
    volume: 0.0,
    emoji: "",
};

catalogue! {
    /// What is being held.
    ///
    /// [`ItemKind::ALL`] is the order an [`Inventory`](super::inventory::Inventory)
    /// keeps its count per kind in, and its debug view lists them in — never a
    /// hash order. Saved by [`ItemKind::name`].
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum ItemKind: ItemDef {
        Food = 0 => ItemDef {
            name: "food",
            consume_seconds: CHEW_SECONDS,
            nutrition: MEAL,
            taste: TASTY,
            mass: MEAL_MASS,
            volume: MEAL_VOLUME,
            emoji: "🍔",
            ..CARRIED
        },
        Water = 1 => ItemDef {
            name: "water",
            consume_seconds: SIP_SECONDS,
            hydration: DRINK,
            mass: DRINK_MASS,
            volume: DRINK_VOLUME,
            emoji: "💧",
            ..CARRIED
        },
        /// A piece of scrap metal, salvaged from a heap in the wasteland
        /// (`brain::tasks::Salvage`). Nothing a body can use: it is carried.
        Scrap = 2 => ItemDef { name: "scrap metal", mass: SCRAP_MASS, volume: SCRAP_VOLUME, emoji: "🔩", ..CARRIED },
        /// The dead, taken up by an undertaker (`brain::tasks::TakeBody`) and
        /// carried to a crematorium. Nothing a body can use.
        Body = 3 => ItemDef {
            name: "body",
            consume_seconds: LAY_DOWN_SECONDS,
            mass: BODY_MASS,
            volume: BODY_VOLUME,
            emoji: "⚰",
            ..CARRIED
        },
        /// An ingot of steel, smelted from scrap metal in the steel factory
        /// (`brain::tasks::Smelt`). Nothing a body can use: it is carried.
        Steel = 4 => ItemDef { name: "steel", mass: STEEL_MASS, volume: STEEL_VOLUME, emoji: "⛓", ..CARRIED },
        /// A sheaf of wheat, harvested from a crop in the fields
        /// (`brain::tasks::Harvest`) and stored at the farm. Not food yet: it
        /// is carried.
        Wheat = 5 => ItemDef { name: "wheat", mass: WHEAT_MASS, volume: WHEAT_VOLUME, emoji: "🌾", ..CARRIED },
    }
}

impl ItemKind {
    /// How it reads in a debug view.
    pub const fn name(self) -> &'static str {
        self.def().name
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
        self.def().consume_seconds
    }

    /// How nourishing it is, on the hunger scale.
    pub const fn nutrition(self) -> f32 {
        self.def().nutrition
    }

    /// How much water is in it, on the thirst scale.
    pub const fn hydration(self) -> f32 {
        self.def().hydration
    }

    /// How much of a treat it is, on the satisfaction scale. Zero for
    /// something with no flavour to speak of — and something with no flavour
    /// is nothing to tire of either.
    pub const fn taste(self) -> f32 {
        self.def().taste
    }

    /// What it weighs, in kilograms.
    ///
    /// Counted against a unit's mass limit wherever it is —  in a hand or
    /// stowed away — because weight is carried either way.
    pub const fn mass(self) -> f32 {
        self.def().mass
    }

    /// How much room it takes up, in litres.
    ///
    /// Counted only against what is **stowed**: space is about packing things
    /// away, and something held in a hand is not packed. See
    /// [`Inventory`](super::inventory::Inventory).
    pub const fn volume(self) -> f32 {
        self.def().volume
    }

    /// The emoji it is drawn as in a hand.
    pub const fn emoji(self) -> &'static str {
        self.def().emoji
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

    /// Every kind has a name of its own, which is what a save keeps, and
    /// something to be drawn as.
    #[test]
    fn every_kind_is_named_once_and_drawn() {
        for (index, kind) in ItemKind::ALL.into_iter().enumerate() {
            assert!(!kind.name().is_empty() && !kind.emoji().is_empty(), "{kind:?}");
            assert!(ItemKind::ALL[..index].iter().all(|other| other.name() != kind.name()), "{}", kind.name());
            assert_eq!(ItemKind::from_name(kind.name()), Some(kind));
        }
    }
}
