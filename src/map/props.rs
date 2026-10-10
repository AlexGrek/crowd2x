//! Props and what they mean to somebody trying to walk through them.
//!
//! The same shape as [`super::terrain`]: a static catalogue keyed by the name a
//! map file stores, holding what a prop *is* to the simulation — for now only
//! whether it can be walked through — and no art. The editor's props palette
//! carries the art under the same names, and `editor::props` has tests that
//! the two lists still line up. What a prop is *for* (a fridge is food) is
//! `sim::feature`'s, one level up.
//!
//! # A prop blocks one cell
//!
//! The cell its centre falls in ([`Object::cell`](super::Object::cell)), and
//! no other. Passability is a fact about whole cells, and that is the rule a
//! body follows too, so it is the one a prop follows: a tall crate whose art
//! overhangs the cell above it does not wall that cell off, and a prop placed
//! on a cell boundary blocks the one side its centre landed on.

use super::terrain::Passability;
use super::ObjectKind;

/// One entry in the prop catalogue.
pub struct Prop {
    pub name: &'static str,
    pub passability: Passability,
}

impl Prop {
    const fn blocking(name: &'static str) -> Self {
        Self {
            name,
            passability: Passability::Impassable,
        }
    }

    const fn walkable(name: &'static str) -> Self {
        Self {
            name,
            passability: Passability::Passable,
        }
    }
}

/// Every prop the editor can place. Nearly everything placeable is furniture
/// somebody would have to climb over, so it blocks; the doors are the
/// exception.
pub const PROPS: &[Prop] = &[
    Prop::blocking("bed 1"),
    Prop::blocking("bed 2"),
    Prop::blocking("bed 3"),
    Prop::blocking("bed 4"),
    Prop::blocking("bed 5"),
    Prop::blocking("bed 6"),
    Prop::blocking("toilet"),
    Prop::blocking("trash can"),
    Prop::blocking("pipe"),
    Prop::blocking("fire"),
    Prop::blocking("crate"),
    Prop::blocking("crate tall"),
    Prop::blocking("fridge"),
    Prop::blocking("computer"),
    // The two ends of the networks under the floor (`super::utilities`):
    // each stands on the street *and* in its network, a box of humming
    // iron and a manhole cover nobody walks across.
    Prop::blocking("transformer"),
    Prop::blocking("sewer"),
    // A door is walked through, so the map lets a route through one. Its
    // leaf is what stops a body, and a leaf opens and shuts during play, so
    // it is the simulation's (`sim::door`), not the map's; a house door is
    // locked to whoever owns the room behind it (`sim::property`).
    Prop::walkable("door"),
    Prop::walkable("house door"),
];

impl ObjectKind {
    /// The catalogue entry for a prop of this kind, or `None` for one this
    /// build does not know.
    pub fn prop(&self) -> Option<&'static Prop> {
        PROPS.iter().find(|prop| prop.name == self.as_str())
    }

    /// Whether a prop of this kind lets anyone stand in its cell. **A prop
    /// this build does not know blocks**, for the reason unknown terrain does:
    /// an agent that cannot be told what something is should walk round it,
    /// not through it.
    pub fn prop_passability(&self) -> Passability {
        self.prop().map_or(Passability::Impassable, |prop| prop.passability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fridge_blocks() {
        assert_eq!(ObjectKind::new("fridge").prop_passability(), Passability::Impassable);
    }

    #[test]
    fn a_door_does_not_block_its_cell() {
        for door in ["door", "house door"] {
            assert_eq!(ObjectKind::new(door).prop_passability(), Passability::Passable, "{door}");
        }
    }

    #[test]
    fn a_prop_this_build_does_not_know_blocks() {
        let from_a_newer_map = ObjectKind::new("hat stand");
        assert!(from_a_newer_map.prop().is_none());
        assert_eq!(from_a_newer_map.prop_passability(), Passability::Impassable);
    }

    /// Names are the key a map file stores a prop under.
    #[test]
    fn every_prop_has_its_own_name() {
        let mut names: Vec<&str> = PROPS.iter().map(|prop| prop.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate prop name");
    }
}
