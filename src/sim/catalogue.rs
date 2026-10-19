//! [`catalogue!`]: a kind enum and everything about each kind, in one list.
//!
//! The simulation names its kinds — items, features, goals, jobs — as enums,
//! so a typo is a compile error and a kind is a `Copy` byte. What each kind
//! *is* (its name, what it weighs, where it is worked) is a row of data. Written
//! as a `match` per property, a new kind is an arm in every one of them, spread
//! down a file and into the save schema; written with this macro it is **one
//! entry**: the variant, the number a save keeps it as, and its row.
//!
//! ```ignore
//! catalogue! {
//!     /// What is being held.
//!     #[derive(Clone, Copy, PartialEq, Eq, Debug)]
//!     pub enum ItemKind: ItemDef {
//!         Food = 0 => ItemDef { name: "food", .. },
//!         /// An ingot of steel.
//!         Steel = 4 => ItemDef { name: "steel", .. },
//!     }
//! }
//! ```
//!
//! gives the enum (`#[repr(u8)]`, the numbers its discriminants), and:
//!
//! * `COUNT` and `ALL`, every kind in the order listed;
//! * `def()`, the kind's row, a `&'static` borrowed in a `const fn`;
//! * `number()` and `from_number()`: what a save writes, and how it reads it
//!   back — `None` for a number this build has no kind for, which a load
//!   refuses.
//!
//! **The numbers are the save format.** They run 0, 1, 2, ... in the order
//! listed — checked at compile time, since `ALL[i]`'s number is the index an
//! array per kind is kept at — so a new kind goes **on the end**, with the next
//! number, and a kind is never taken out or renumbered while a save could
//! name it.

/// See the [module docs](self).
macro_rules! catalogue {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident: $def:ty {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $number:literal => $row:expr
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr(u8)]
        $vis enum $name {
            $( $(#[$variant_meta])* $variant = $number, )*
        }

        impl $name {
            /// How many kinds there are.
            pub const COUNT: usize = [$($name::$variant),*].len();

            /// Every kind, in the order listed — which is also the order of
            /// their numbers.
            pub const ALL: [$name; $name::COUNT] = [$($name::$variant),*];

            /// Everything about this kind: its row in the catalogue.
            pub const fn def(self) -> &'static $def {
                match self {
                    $( $name::$variant => &const { $row }, )*
                }
            }

            /// The number a save keeps this kind as.
            #[allow(dead_code)]
            pub const fn number(self) -> i32 {
                self as i32
            }

            /// The kind a save numbered `number`, or `None` for one this build
            /// does not have.
            #[allow(dead_code)]
            pub const fn from_number(number: i32) -> Option<$name> {
                match number {
                    $( $number => Some($name::$variant), )*
                    _ => None,
                }
            }
        }

        const _: () = {
            let mut i = 0;
            while i < $name::COUNT {
                assert!(
                    $name::ALL[i] as usize == i,
                    concat!(stringify!($name), ": number the kinds 0, 1, 2, ... in the order they are listed")
                );
                i += 1;
            }
        };
    };
}

pub(crate) use catalogue;
