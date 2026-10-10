//! Grid layers: one small value per cell, authored, over or under the
//! terrain — [`GridLayer`].
//!
//! The terrain says what a cell *is* and the object layers what stands in it;
//! a grid layer is everything else a map paints cell by cell. The ceiling
//! over a room is one, and so are the two networks under the floor: the
//! electrical grid and the sewer pipes. They share one shape — a byte per
//! cell, a short alphabet of what that byte may be, a row of characters per
//! map row in the file — so a new one is a variant here, its alphabet, and
//! whatever reads it, and the file format, the editor and the overlay that
//! draws it pick it up from this table.
//!
//! None of them is in anybody's way: passability is the terrain's and the
//! props' business alone. What they decide is elsewhere — the sun under a
//! roof (`crate::lighting`), and whether a fridge has power or a toilet a
//! drain ([`super::utilities`]).

/// A per-cell layer of the map that is not the terrain.
///
/// The discriminants are the storage order in `Map`, so a layer is found by
/// casting rather than by searching.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GridLayer {
    /// Whether a cell has a roof over it: [`ceiling`].
    Ceiling = 0,
    /// The electrical grid under the floor: [`power`].
    Power = 1,
    /// The sewer pipes under the floor: [`water`].
    Water = 2,
}

impl GridLayer {
    pub const ALL: [GridLayer; 3] = [GridLayer::Ceiling, GridLayer::Power, GridLayer::Water];
    pub const COUNT: usize = GridLayer::ALL.len();

    /// The key this layer is stored under in a map file. Renaming one is a
    /// format change, not a cosmetic edit — `"ceiling"` predates the rest and
    /// is kept for exactly that reason.
    pub const fn name(self) -> &'static str {
        match self {
            GridLayer::Ceiling => "ceiling",
            GridLayer::Power => "power",
            GridLayer::Water => "water",
        }
    }

    pub fn from_name(name: &str) -> Option<GridLayer> {
        GridLayer::ALL.into_iter().find(|layer| layer.name() == name)
    }

    /// One character per value, value 0 — nothing there — first. What a
    /// cell is written as in a file, so these are a format too: a row of
    /// `.` and `#` reads as the building it roofs, and a row of `.`, `-`,
    /// `=` and `B` as the wiring it carries.
    pub const fn alphabet(self) -> &'static [char] {
        match self {
            GridLayer::Ceiling => &['.', '#'],
            GridLayer::Power => &['.', '-', '=', 'B'],
            GridLayer::Water => &['.', 'o'],
        }
    }

    /// Below the floor rather than above it: drawn as an x-ray over the
    /// world, and never seen by anybody in it.
    pub const fn is_underground(self) -> bool {
        !matches!(self, GridLayer::Ceiling)
    }

    /// The character `value` is written as, or `None` past the alphabet.
    pub fn char_of(self, value: u8) -> Option<char> {
        self.alphabet().get(value as usize).copied()
    }

    /// The value a character stands for, or `None` for one not in the
    /// alphabet.
    pub fn value_of(self, c: char) -> Option<u8> {
        self.alphabet().iter().position(|&known| known == c).map(|v| v as u8)
    }
}

/// The ceiling's values.
pub mod ceiling {
    pub const OPEN: u8 = 0;
    pub const ROOFED: u8 = 1;
}

/// The electrical grid's values: two voltages that never touch except
/// through a box.
///
/// A **power line** carries the transformer's high voltage along the
/// streets; a **distribution box** takes it in and gives out the low voltage
/// that the **wiring** carries to whatever plugs in. A cell holds one of
/// them, so a line and a wire cannot cross in one cell — they meet only in a
/// box, which is the point of a box. See [`super::utilities`] for what is
/// connected to what.
pub mod power {
    pub const NONE: u8 = 0;
    /// Low voltage, from a box to whatever uses it.
    pub const WIRING: u8 = 1;
    /// High voltage, from a transformer to the boxes.
    pub const LINE: u8 = 2;
    /// Where a line meets wiring.
    pub const BOX: u8 = 3;
}

/// The sewer's values: a pipe, or none.
pub mod water {
    pub const NONE: u8 = 0;
    pub const PIPE: u8 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_has_its_own_name_and_an_alphabet_starting_with_nothing() {
        let mut names: Vec<&str> = GridLayer::ALL.iter().map(|layer| layer.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), GridLayer::COUNT);
        for layer in GridLayer::ALL {
            assert_eq!(layer.char_of(0), Some('.'), "{layer:?}: an empty cell reads as '.'");
            assert_eq!(GridLayer::from_name(layer.name()), Some(layer));
            for (value, &c) in layer.alphabet().iter().enumerate() {
                assert_eq!(layer.value_of(c), Some(value as u8), "{layer:?} {c:?}");
            }
        }
    }

    #[test]
    fn the_layers_are_stored_in_the_order_they_are_listed() {
        for (index, layer) in GridLayer::ALL.into_iter().enumerate() {
            assert_eq!(layer as usize, index);
        }
    }

    #[test]
    fn the_values_fit_their_alphabets() {
        assert_eq!(GridLayer::Ceiling.char_of(ceiling::ROOFED), Some('#'));
        assert_eq!(GridLayer::Power.char_of(power::BOX), Some('B'));
        assert_eq!(GridLayer::Water.char_of(water::PIPE), Some('o'));
        assert_eq!(GridLayer::Water.char_of(2), None);
    }
}
