//! Things a unit can hold: [`ItemKind`].
//!
//! Several kinds, one hand. That is the honest minimum for "take food out of
//! the fridge, then eat it" to be two steps rather than one, and it is not an
//! inventory — a `Vec` of items per unit is an allocation per unit, and there
//! is nothing yet that wants to hold two things.
//!
//! An item says what it is made of — how nourishing, how much water — and not
//! what that does to whoever consumes it. That is the body's business: the
//! processes in [`super::biology`] read these numbers when they hear it was
//! eaten, so a drink filling a bladder is the bladder's doing, not the
//! water's.

/// How nourishing one meal is: the hunger it takes away.
pub const MEAL: f32 = 60.0;

/// How much water one drink is: the thirst it takes away.
pub const DRINK: f32 = 60.0;

/// Seconds spent eating food.
pub const CHEW_SECONDS: f32 = 2.0;

/// Seconds spent drinking water: half as long as a meal.
pub const SIP_SECONDS: f32 = 1.0;

/// What is being held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Food,
    Water,
}

impl ItemKind {
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
}
