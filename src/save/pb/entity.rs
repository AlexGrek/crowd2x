//! `entities/NNNN.pb`: the entity arena, [`crate::save::SLOTS_PER_FILE`]
//! slots a file, and everything one entity is — but its mind, which is
//! [`super::brain`].

use super::{Brain, Cell};

/// One file's worth of the arena. Holes are left out; [`super::Arena`] says
/// how many slots there are.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EntityShard {
    #[prost(message, repeated, tag = "1")]
    pub slots: Vec<SlotRecord>,
}

/// One live entity, and where in the arena it lives.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SlotRecord {
    #[prost(uint32, tag = "1")]
    pub slot: u32,
    /// When it arrived, as a count of the arrivals before it.
    #[prost(uint64, tag = "2")]
    pub serial: u64,
    /// Whether the occupancy layer names it as the one standing in its cell.
    /// Not everybody who stands in a cell holds it: spawning can stack two
    /// in one, and only the first holds it.
    #[prost(bool, tag = "3")]
    pub holds_cell: bool,
    /// What it is. Must agree with the kind its uid says.
    #[prost(oneof = "Kind", tags = "4, 5, 6")]
    pub kind: Option<Kind>,
}

#[derive(Clone, PartialEq, ::prost::Oneof)]
pub enum Kind {
    #[prost(message, tag = "4")]
    Human(Human),
    #[prost(message, tag = "5")]
    Dog(Dog),
    #[prost(message, tag = "6")]
    Deathdog(Deathdog),
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Human {
    #[prost(message, optional, tag = "1")]
    pub walker: Option<Walker>,
    #[prost(message, optional, tag = "2")]
    pub brain: Option<Brain>,
    #[prost(message, optional, tag = "3")]
    pub biology: Option<Biology>,
    #[prost(message, optional, tag = "4")]
    pub identity: Option<Identity>,
    #[prost(message, optional, tag = "5")]
    pub inventory: Option<Inventory>,
    #[prost(message, repeated, tag = "6")]
    pub talents: Vec<TalentValue>,
    #[prost(message, optional, tag = "7")]
    pub schedule: Option<Schedule>,
    /// Stable outfit name. Absent in older saves: recover the seeded outfit.
    #[prost(string, optional, tag = "8")]
    pub clothing: Option<String>,
    /// Older humans remain commoners.
    #[prost(bool, tag = "9")]
    pub noble: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Dog {
    #[prost(message, optional, tag = "1")]
    pub walker: Option<Walker>,
    #[prost(message, optional, tag = "2")]
    pub brain: Option<Brain>,
    #[prost(enumeration = "Facing", tag = "3")]
    pub facing: i32,
    #[prost(message, optional, tag = "4")]
    pub identity: Option<Identity>,
    /// How hungry it is and what it has yet to pass (`crate::sim::pets::Gut`).
    /// Absent, in a save from before pets: a full belly.
    #[prost(message, optional, tag = "5")]
    pub gut: Option<Gut>,
    /// Absent in older saves: full health with natural healing enabled.
    #[prost(message, optional, tag = "6")]
    pub biology: Option<Biology>,
}

/// A dog's belly (`crate::sim::pets::Gut`): hunger, what is still being
/// digested and the bowel, each 0 to 100.
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Gut {
    #[prost(float, tag = "1")]
    pub hunger: f32,
    #[prost(float, tag = "2")]
    pub digesting: f32,
    #[prost(float, tag = "3")]
    pub bowel: f32,
}

/// A deathdog: a body, a mind, which way it faces and its background
/// timers. No name, no needs, nothing carried.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Deathdog {
    #[prost(message, optional, tag = "1")]
    pub walker: Option<Walker>,
    #[prost(message, optional, tag = "2")]
    pub brain: Option<Brain>,
    #[prost(enumeration = "Facing", tag = "3")]
    pub facing: i32,
    #[prost(message, optional, tag = "4")]
    pub schedule: Option<Schedule>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Facing {
    Left = 0,
    Right = 1,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Body {
    #[prost(fixed64, tag = "1")]
    pub uid: u64,
    /// Position in cell units. `float`, so it comes back to the bit.
    #[prost(float, tag = "2")]
    pub x: f32,
    #[prost(float, tag = "3")]
    pub y: f32,
    #[prost(bool, tag = "4")]
    pub frozen: bool,
    /// The property it holds the key to.
    #[prost(uint32, optional, tag = "5")]
    pub home: Option<u32>,
    /// Dead: it thinks and moves no more. Absent, alive.
    #[prost(bool, tag = "6")]
    pub dead: bool,
    /// Running for its life (`Body::is_panicking`). Absent, calm — all
    /// anybody was before there were deathdogs.
    #[prost(bool, tag = "7")]
    pub panicking: bool,
    /// The house it spawned in: everybody born into the same one is family
    /// (`crate::sim::relationships`). Absent — and in every save from before
    /// there were families — is nobody's.
    #[prost(uint32, optional, tag = "8")]
    pub family: Option<u32>,
    /// Missing in older saves means clean.
    #[prost(message, optional, tag = "9")]
    pub dirt: Option<super::Dirt>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Walker {
    #[prost(message, optional, tag = "1")]
    pub body: Option<Body>,
    /// Cells per second.
    #[prost(float, tag = "2")]
    pub speed: f32,
    #[prost(message, optional, tag = "3")]
    pub path: Option<Path>,
    /// A far route being searched for.
    #[prost(message, optional, tag = "4")]
    pub request: Option<RouteRequest>,
    /// The unfinished search behind `request`, when it kept its own.
    #[prost(message, optional, tag = "5")]
    pub search: Option<PathSearch>,
    /// How much of a hurry it is in, 0 to 1 (`Walker::hurry`); absent = 0,
    /// its own pace, which is all anybody walked at before.
    #[prost(float, tag = "6")]
    pub hurry: f32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Path {
    /// The far route, packed `x, y`.
    #[prost(sint32, repeated, tag = "1")]
    pub steps: Vec<i32>,
    #[prost(uint32, tag = "2")]
    pub next: u32,
    /// A detour in front of it, packed `x, y`.
    #[prost(sint32, repeated, tag = "3")]
    pub patch: Vec<i32>,
    #[prost(uint32, tag = "4")]
    pub patch_next: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RouteRequest {
    #[prost(message, optional, tag = "1")]
    pub start: Option<Cell>,
    /// One to four cells, packed `x, y`.
    #[prost(sint32, repeated, tag = "2")]
    pub goals: Vec<i32>,
    /// What to try when none of `goals` can be reached; absent for nothing.
    #[prost(message, optional, tag = "3")]
    pub fallback: Option<Destinations>,
    #[prost(uint32, optional, tag = "4")]
    pub key: Option<u32>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Destinations {
    /// One to four cells, packed `x, y`.
    #[prost(sint32, repeated, tag = "1")]
    pub cells: Vec<i32>,
}

/// A far search stopped partway by the route budget, to carry on from
/// exactly where it was: the A* frontier and every cell reached so far.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PathSearch {
    #[prost(sint32, tag = "1")]
    pub window_x: i32,
    #[prost(sint32, tag = "2")]
    pub window_y: i32,
    #[prost(sint32, tag = "3")]
    pub window_width: i32,
    #[prost(sint32, tag = "4")]
    pub window_height: i32,
    #[prost(message, optional, tag = "5")]
    pub start: Option<Cell>,
    /// Packed `x, y`.
    #[prost(sint32, repeated, tag = "6")]
    pub goals: Vec<i32>,
    #[prost(uint32, tag = "7")]
    pub minimum: u32,
    /// The open set, in no particular order.
    #[prost(message, repeated, tag = "8")]
    pub open: Vec<Candidate>,
    /// Every cell this search has reached.
    #[prost(message, repeated, tag = "9")]
    pub seen: Vec<Visit>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Candidate {
    #[prost(uint64, tag = "1")]
    pub f: u64,
    #[prost(uint64, tag = "2")]
    pub h: u64,
    #[prost(sint32, tag = "3")]
    pub x: i32,
    #[prost(sint32, tag = "4")]
    pub y: i32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Visit {
    /// The cell's index in the window, row-major.
    #[prost(uint32, tag = "1")]
    pub index: u32,
    #[prost(uint64, tag = "2")]
    pub cost: u64,
    #[prost(sint32, tag = "3")]
    pub from_x: i32,
    #[prost(sint32, tag = "4")]
    pub from_y: i32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Biology {
    #[prost(message, optional, tag = "1")]
    pub stats: Option<Stats>,
    /// The processes switched off. Off and not on, so a process added later
    /// comes back running, as it would in a unit spawned today.
    #[prost(enumeration = "ProcessId", repeated, tag = "2")]
    pub off: Vec<i32>,
    #[prost(message, repeated, tag = "3")]
    pub familiarity: Vec<Familiarity>,
    /// What a drink has still to put in the bladder.
    #[prost(float, tag = "4")]
    pub bladder_on_its_way: f32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Stats {
    #[prost(float, tag = "1")]
    pub health: f32,
    #[prost(float, tag = "2")]
    pub stamina: f32,
    #[prost(float, tag = "3")]
    pub fun: f32,
    #[prost(float, tag = "4")]
    pub hunger: f32,
    #[prost(float, tag = "5")]
    pub thirst: f32,
    #[prost(float, tag = "6")]
    pub bladder: f32,
    #[prost(float, tag = "7")]
    pub mental_health: f32,
    #[prost(float, tag = "8")]
    pub attention: f32,
    #[prost(float, tag = "9")]
    pub satisfaction: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum ProcessId {
    Hunger = 0,
    Thirst = 1,
    Bladder = 2,
    Fun = 3,
    Energy = 4,
    Satisfaction = 5,
    Health = 6,
    Mind = 7,
}

/// How used to one experience a body is.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Familiarity {
    #[prost(enumeration = "ExperienceKind", tag = "1")]
    pub kind: i32,
    /// For a taste, the item's name.
    #[prost(string, tag = "2")]
    pub item: String,
    #[prost(float, tag = "3")]
    pub value: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum ExperienceKind {
    Taste = 0,
    Entertainment = 1,
    Company = 2,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Identity {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(enumeration = "Gender", tag = "2")]
    pub gender: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Gender {
    Male = 0,
    Female = 1,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Inventory {
    /// What is in the hand, by item name; absent for nothing.
    #[prost(string, optional, tag = "1")]
    pub hand: Option<String>,
    /// What is stowed, by item name. Kinds with none are left out.
    #[prost(message, repeated, tag = "2")]
    pub stowed: Vec<ItemCount>,
    /// Kilograms it can carry.
    #[prost(float, tag = "3")]
    pub capacity_mass: f32,
    /// Litres it can stow.
    #[prost(float, tag = "4")]
    pub capacity_volume: f32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ItemCount {
    #[prost(string, tag = "1")]
    pub item: String,
    #[prost(uint32, tag = "2")]
    pub count: u32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct TalentValue {
    #[prost(enumeration = "Talent", tag = "1")]
    pub talent: i32,
    #[prost(uint32, tag = "2")]
    pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Talent {
    Arts = 0,
    Engineering = 1,
    Communication = 2,
    Management = 3,
    Athleticism = 4,
    Military = 5,
    Beauty = 6,
    Responsibility = 7,
    Precision = 8,
}

/// A unit's background timers (`crate::sim::background`).
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Schedule {
    #[prost(fixed64, tag = "1")]
    pub seed: u64,
    #[prost(message, repeated, tag = "2")]
    pub slots: Vec<ScheduleSlot>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct ScheduleSlot {
    #[prost(enumeration = "Priority", tag = "1")]
    pub priority: i32,
    /// Ticks until it is next due.
    #[prost(uint32, tag = "2")]
    pub left: u32,
    /// World seconds gone by since it last ran.
    #[prost(float, tag = "3")]
    pub pending: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Priority {
    High = 0,
    Med = 1,
    Low = 2,
    VeryLow = 3,
}
