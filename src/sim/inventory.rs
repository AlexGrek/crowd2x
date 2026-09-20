//! What a unit is carrying: [`Inventory`], and the [`Capacity`] that limits it.
//!
//! Two slots of a different nature, and the difference is the whole design:
//!
//! * **The hand** — one item, the thing being used right now. Taking food out
//!   of a fridge puts it here; eating it takes it from here. It is *not* part
//!   of the inventory proper.
//! * **What is stowed** — everything else being carried, and what the limits
//!   are really about.
//!
//! # Two limits, and which slot each one counts
//!
//! | | the hand | what is stowed |
//! | --- | --- | --- |
//! | **mass** (kg) | counted | counted |
//! | **volume** (litres) | not counted | counted |
//!
//! **Weight is carried wherever it is**, so a full hand eats into how much can
//! still be stowed. **Space is about packing things away**, and something held
//! in a hand is not packed — a unit with its hands full has not lost any
//! shelf room, it has lost a hand.
//!
//! Both limits genuinely bind, which is why there are two of them: food is
//! bulky for its weight and water is dense, so a unit runs out of *space*
//! carrying meals and out of *strength* carrying drinks. See
//! [`crate::sim::item`] for those numbers.
//!
//! # The limits govern stowing, and never the hand
//!
//! [`Inventory::stow`] refuses an item that would put either total over the
//! limit. [`Inventory::set_hand`] refuses nothing, and that asymmetry is
//! deliberate: a unit that cannot pick up food because it is already laden is
//! a unit that cannot eat, and the way out of being overloaded is *through*
//! the hand — you consume or put down what you are holding. A hand that could
//! be refused would deadlock a hungry unit whose pockets are full, and nothing
//! in the brain would be able to tell why.
//!
//! # A count per kind, not a list of things
//!
//! [`ItemKind`] is fieldless: two portions of food are the same item in every
//! way the simulation can tell them apart. So an inventory is one `u8` per
//! kind — `Copy`, inline in the unit, no allocation in a tick, and no slot
//! count to impose a third limit nobody asked for. The only limits are the two
//! above. An item that grows state of its own is a change to [`ItemKind`]
//! first, and this follows it.

use super::item::ItemKind;

/// How much a unit can carry: a mass limit and a volume limit.
///
/// Per unit rather than a global constant, so that a stronger unit, a child or
/// somebody holding a bag can differ without any of this changing shape. Two
/// `f32`s inline; nothing dereferences a pointer to find out how much it can
/// carry.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Capacity {
    /// Kilograms, over everything carried — the hand included.
    pub mass: f32,
    /// Litres, over what is stowed. The hand does not count against it.
    pub volume: f32,
}

impl Capacity {
    /// What a person can carry in their arms and pockets, with no bag.
    ///
    /// Ten kilos is a load somebody can walk about with all day; twelve litres
    /// is about as much as arms and pockets hold. Against the item sizes in
    /// [`crate::sim::item`] that is twelve boxed meals (space runs out first)
    /// or twenty glasses of water (weight does).
    pub const HUMAN: Capacity = Capacity {
        mass: 10.0,
        volume: 12.0,
    };
}

/// Everything a unit is carrying: one hand, and what is stowed away.
///
/// See the module docs for which limit counts which slot, and why the hand is
/// never refused.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Inventory {
    /// What is in its hand. One hand, one thing.
    hand: Option<ItemKind>,
    /// How many of each [`ItemKind`] are stowed, indexed by the kind itself.
    counts: [u8; ItemKind::COUNT],
    capacity: Capacity,
}

impl Inventory {
    /// The most of any one kind that can be stowed, whatever the limits say.
    ///
    /// The ceiling of the `u8` a count is kept in. With today's items and
    /// [`Capacity::HUMAN`] the mass and volume limits bite long before this
    /// does — it is here so that a future feather-light item overflows into a
    /// refusal rather than into wrapping round to none.
    pub const MOST_OF_A_KIND: u8 = u8::MAX;

    /// An empty inventory with these limits.
    pub const fn new(capacity: Capacity) -> Inventory {
        Inventory {
            hand: None,
            counts: [0; ItemKind::COUNT],
            capacity,
        }
    }

    /// An empty inventory a person could carry.
    pub const fn human() -> Inventory {
        Inventory::new(Capacity::HUMAN)
    }

    /// What is in its hand.
    pub const fn hand(&self) -> Option<ItemKind> {
        self.hand
    }

    /// Put something in its hand, or empty it.
    ///
    /// **Never refused**, even when it puts the unit over its mass limit — the
    /// module docs say why. Whatever was there is returned, which is the caller's
    /// to deal with; a task that would drop something on the floor should stow
    /// it or consume it first, since nothing picks up what is dropped.
    pub const fn set_hand(&mut self, item: Option<ItemKind>) -> Option<ItemKind> {
        let was = self.hand;
        self.hand = item;
        was
    }

    /// How much this unit can carry.
    pub const fn capacity(&self) -> Capacity {
        self.capacity
    }

    /// How many of `item` are stowed. What is in the hand is not stowed, and
    /// does not count here.
    pub const fn count(&self, item: ItemKind) -> u8 {
        self.counts[item as usize]
    }

    /// Whether anything at all is being carried, hand included.
    pub fn is_empty(&self) -> bool {
        self.hand.is_none() && self.counts.iter().all(|&count| count == 0)
    }

    /// How many items are stowed, of every kind together.
    pub fn stowed_items(&self) -> u32 {
        self.counts.iter().map(|&count| count as u32).sum()
    }

    /// Every kind that is stowed, with how many, in [`ItemKind::ALL`] order.
    ///
    /// Kinds with none of them are skipped, so this is what a debug view or a
    /// panel lists.
    pub fn stowed(&self) -> impl Iterator<Item = (ItemKind, u8)> + '_ {
        ItemKind::ALL
            .into_iter()
            .map(|kind| (kind, self.count(kind)))
            .filter(|&(_, count)| count > 0)
    }

    /// What everything being carried weighs, in kilograms — **the hand
    /// included**.
    pub fn mass(&self) -> f32 {
        let hand = self.hand.map_or(0.0, ItemKind::mass);
        hand + self.fold(ItemKind::mass)
    }

    /// What is stowed, in litres. The hand is not counted: see the module docs.
    pub fn volume(&self) -> f32 {
        self.fold(ItemKind::volume)
    }

    /// The stowed total of whatever an item measures.
    fn fold(&self, of: fn(ItemKind) -> f32) -> f32 {
        ItemKind::ALL
            .into_iter()
            .map(|kind| of(kind) * self.count(kind) as f32)
            .sum()
    }

    /// Whether one more `item` would still fit, by both limits and the count.
    ///
    /// Asked before [`Inventory::stow`] by anything that wants to know without
    /// trying — `stow` checks this itself, so there is no way to get an item in
    /// past it.
    pub fn room_for(&self, item: ItemKind) -> bool {
        self.count(item) < Inventory::MOST_OF_A_KIND
            && self.mass() + item.mass() <= self.capacity.mass
            && self.volume() + item.volume() <= self.capacity.volume
    }

    /// Stow one `item`. `false`, and nothing changed, when it does not fit.
    ///
    /// Refused rather than squeezed in: a limit that bends is not a limit, and
    /// a unit that is over its own is one nothing can reason about.
    #[must_use = "an inventory with no room refuses the item"]
    pub fn stow(&mut self, item: ItemKind) -> bool {
        if !self.room_for(item) {
            return false;
        }
        self.counts[item as usize] += 1;
        true
    }

    /// Take one `item` back out of what is stowed. `false`, and nothing
    /// changed, when there was none to take.
    ///
    /// It does not go into the hand: where it goes is the caller's to say, and
    /// a hand that was quietly overwritten would lose whatever was in it.
    #[must_use = "there may have been none to take"]
    pub fn take_out(&mut self, item: ItemKind) -> bool {
        if self.counts[item as usize] == 0 {
            return false;
        }
        self.counts[item as usize] -= 1;
        true
    }

    /// What is being carried, as a debug view reads it: `2 food, 1 water`, or
    /// `nothing`.
    ///
    /// Allocates, like every `debug_fields` in the simulation, and for the same
    /// reason: it is asked about the one unit somebody has selected and never
    /// in a tick.
    pub fn describe_stowed(&self) -> String {
        if self.stowed_items() == 0 {
            return "nothing".to_string();
        }
        self.stowed()
            .map(|(kind, count)| format!("{count} {}", kind.name()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// How laden it is, against what it can take: `3.5 / 10.0 kg, 6.0 / 12.0 L`.
    pub fn describe_load(&self) -> String {
        format!(
            "{:.1} / {:.1} kg, {:.1} / {:.1} L",
            self.mass(),
            self.capacity.mass,
            self.volume(),
            self.capacity.volume
        )
    }

    /// What a unit would tell a debugger about what it is carrying.
    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "holding",
                self.hand.map_or("nothing", ItemKind::name).to_string(),
            ),
            ("carrying", self.describe_stowed()),
            ("load", self.describe_load()),
        ]
    }
}

// No `Default`, on purpose: it would have to pick a capacity, and the only
// honest one to pick is a person's. A crate-carrying machine that got a
// human's limits by writing `..Default::default()` would be wrong in a way
// nothing would report. Say which capacity you mean — `Inventory::human()` or
// `Inventory::new(..)`.

#[cfg(test)]
mod tests {
    use super::*;

    /// Room for exactly two food (by volume) and no more, so the limits are
    /// reached in a line or two rather than in twenty.
    fn small() -> Inventory {
        Inventory::new(Capacity {
            mass: 10.0,
            volume: 2.0,
        })
    }

    #[test]
    fn a_new_inventory_is_empty_and_weighs_nothing() {
        let inventory = Inventory::human();
        assert!(inventory.is_empty());
        assert_eq!(inventory.mass(), 0.0);
        assert_eq!(inventory.volume(), 0.0);
        assert_eq!(inventory.stowed_items(), 0);
        assert_eq!(inventory.count(ItemKind::Food), 0);
    }

    #[test]
    fn stowing_adds_to_both_totals_and_taking_out_takes_them_away_again() {
        let mut inventory = Inventory::human();
        assert!(inventory.stow(ItemKind::Food));
        assert!(inventory.stow(ItemKind::Food));
        assert!(inventory.stow(ItemKind::Water));

        assert_eq!(inventory.count(ItemKind::Food), 2);
        assert_eq!(inventory.count(ItemKind::Water), 1);
        assert_eq!(inventory.stowed_items(), 3);
        assert_eq!(inventory.mass(), 2.0 * ItemKind::Food.mass() + ItemKind::Water.mass());
        assert_eq!(
            inventory.volume(),
            2.0 * ItemKind::Food.volume() + ItemKind::Water.volume()
        );

        assert!(inventory.take_out(ItemKind::Food));
        assert_eq!(inventory.count(ItemKind::Food), 1);
        assert!(!inventory.is_empty());
    }

    #[test]
    fn taking_out_what_is_not_there_changes_nothing() {
        let mut inventory = Inventory::human();
        assert!(!inventory.take_out(ItemKind::Water));
        assert!(inventory.is_empty());
    }

    #[test]
    fn what_is_in_the_hand_is_not_stowed() {
        // The hand is a slot of its own: it is not in the counts, and emptying
        // the inventory does not empty it.
        let mut inventory = Inventory::human();
        assert_eq!(inventory.set_hand(Some(ItemKind::Food)), None);

        assert_eq!(inventory.count(ItemKind::Food), 0);
        assert_eq!(inventory.stowed_items(), 0);
        assert!(!inventory.is_empty(), "it is carrying something");
        assert_eq!(inventory.set_hand(None), Some(ItemKind::Food));
    }

    #[test]
    fn a_full_hand_weighs_against_the_limit_but_takes_up_no_space() {
        // The headline rule: mass counts the hand, volume does not.
        let mut inventory = Inventory::human();
        assert!(inventory.stow(ItemKind::Food));
        let (mass, volume) = (inventory.mass(), inventory.volume());

        let _ = inventory.set_hand(Some(ItemKind::Water));

        assert_eq!(inventory.mass(), mass + ItemKind::Water.mass());
        assert_eq!(inventory.volume(), volume, "a hand takes up no shelf room");
    }

    #[test]
    fn what_is_in_the_hand_leaves_less_room_to_stow_by_weight() {
        // Because the hand counts against mass, filling it has to cost stowing
        // capacity — otherwise "contributes to overall mass" means nothing.
        let mut heavy = Inventory::new(Capacity {
            mass: 1.0,
            volume: 100.0,
        });
        assert!(heavy.room_for(ItemKind::Food));

        let _ = heavy.set_hand(Some(ItemKind::Food));
        assert!(heavy.stow(ItemKind::Food), "1.0 kg holds exactly two 0.5 kg items");
        assert!(!heavy.room_for(ItemKind::Food), "the hand used up the rest");
        assert!(!heavy.stow(ItemKind::Food));
        assert_eq!(heavy.count(ItemKind::Food), 1);
    }

    #[test]
    fn a_hand_is_never_refused_even_over_the_limit() {
        // A unit that cannot pick food up because its pockets are full is a
        // unit that cannot eat. The way out of being laden is through the hand.
        let mut inventory = small();
        while inventory.stow(ItemKind::Food) {}
        assert!(!inventory.room_for(ItemKind::Food), "should be full");

        let _ = inventory.set_hand(Some(ItemKind::Food));
        assert_eq!(inventory.hand(), Some(ItemKind::Food));
        assert!(inventory.mass() > 0.0);
    }

    #[test]
    fn stowing_stops_at_the_volume_limit() {
        let mut inventory = small();
        assert!(inventory.stow(ItemKind::Food));
        assert!(inventory.stow(ItemKind::Food));

        assert!(!inventory.stow(ItemKind::Food), "2.0 L holds two 1.0 L items");
        assert_eq!(inventory.count(ItemKind::Food), 2, "a refusal changes nothing");
        assert!(inventory.volume() <= inventory.capacity().volume);
    }

    #[test]
    fn stowing_stops_at_the_mass_limit() {
        // Lots of room, no strength: the other limit, on its own.
        let mut inventory = Inventory::new(Capacity {
            mass: 1.2,
            volume: 1000.0,
        });
        assert!(inventory.stow(ItemKind::Water));
        assert!(inventory.stow(ItemKind::Water));

        assert!(!inventory.stow(ItemKind::Water), "1.2 kg holds two 0.5 kg items");
        assert_eq!(inventory.count(ItemKind::Water), 2);
        assert!(inventory.mass() <= inventory.capacity().mass);
    }

    /// Both limits are load-bearing for the items that exist today — that is
    /// the reason there are two of them, and a change to the item sizes that
    /// quietly made one of them decoration should fail here.
    #[test]
    fn food_runs_out_of_space_and_water_runs_out_of_strength() {
        let fill = |item| {
            let mut inventory = Inventory::human();
            let mut stowed = 0;
            while inventory.stow(item) {
                stowed += 1;
            }
            (inventory, stowed)
        };

        let (with_food, meals) = fill(ItemKind::Food);
        assert!(
            with_food.volume() + ItemKind::Food.volume() > with_food.capacity().volume,
            "food should be stopped by space, not weight"
        );
        assert_eq!(meals, 12);

        let (with_water, glasses) = fill(ItemKind::Water);
        assert!(
            with_water.mass() + ItemKind::Water.mass() > with_water.capacity().mass,
            "water should be stopped by weight, not space"
        );
        assert_eq!(glasses, 20);
    }

    #[test]
    fn a_debug_view_says_what_is_carried_and_how_laden_it_is() {
        let mut inventory = Inventory::human();
        assert_eq!(inventory.describe_stowed(), "nothing");

        let _ = inventory.set_hand(Some(ItemKind::Water));
        assert!(inventory.stow(ItemKind::Food));
        assert!(inventory.stow(ItemKind::Food));
        assert!(inventory.stow(ItemKind::Water));

        assert_eq!(inventory.describe_stowed(), "2 food, 1 water");
        // 0.5 in hand + 1.0 of food + 0.5 of water = 2.0 kg; the hand is not
        // in the 2.5 L.
        assert_eq!(inventory.describe_load(), "2.0 / 10.0 kg, 2.5 / 12.0 L");

        let fields = inventory.debug_fields();
        assert_eq!(fields[0], ("holding", "water".to_string()));
        assert_eq!(fields[1].1, "2 food, 1 water");
    }

    #[test]
    fn an_inventory_is_copy_and_allocates_nothing_to_carry_things() {
        // It lives inline in a unit and is touched in the tick, so it must not
        // be a Vec. The absence of one is a property of the type; its size is
        // what a test can see.
        let mut inventory = Inventory::human();
        let before = inventory;
        assert!(inventory.stow(ItemKind::Food));
        assert_eq!(before.count(ItemKind::Food), 0, "a copy is a copy");

        assert!(
            std::mem::size_of::<Inventory>() <= 16,
            "an Inventory is {} bytes",
            std::mem::size_of::<Inventory>()
        );
    }
}
