//! Things a unit can hold: [`ItemKind`].
//!
//! One kind and one hand. That is the honest minimum for "take food out of the
//! fridge, then eat it" to be two steps rather than one, and it is not an
//! inventory — a `Vec` of items per unit is an allocation per unit, and there
//! is nothing yet that wants to hold two things.

/// What is being held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Food,
}

impl ItemKind {
    /// How it reads in a debug view.
    pub const fn name(self) -> &'static str {
        match self {
            ItemKind::Food => "food",
        }
    }
}
