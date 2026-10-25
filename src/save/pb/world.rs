//! `world.pb`: the world's own state, everything that is not an entity, the
//! map or a chunk.

use super::Cell;

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct World {
    /// Ticks processed.
    #[prost(uint64, tag = "1")]
    pub tick: u64,
    /// Watched seconds since the world began.
    #[prost(double, tag = "2")]
    pub elapsed: f64,
    /// The id allocator's state: four words of xoshiro256++
    /// (`crate::sim::rng::state_of`).
    #[prost(fixed64, repeated, tag = "3")]
    pub rng: Vec<u64>,
    /// Distribution boxes switched off by hand, packed `x, y`.
    #[prost(sint32, repeated, tag = "4")]
    pub boxes_off: Vec<i32>,
    /// Whether the street light controllers last saw night.
    #[prost(bool, tag = "5")]
    pub street_lights_on: bool,
    /// Light switches that are off, packed `x, y`.
    #[prost(sint32, repeated, tag = "6")]
    pub switches_off: Vec<i32>,
    #[prost(message, repeated, tag = "7")]
    pub fridges: Vec<Fridge>,
    #[prost(message, repeated, tag = "8")]
    pub doors: Vec<Door>,
    /// Which beds have been given to whom.
    #[prost(message, repeated, tag = "9")]
    pub homes: Vec<Home>,
    /// How the entity arena is laid out, beyond who is in it.
    #[prost(message, optional, tag = "10")]
    pub arena: Option<Arena>,
    /// Which desks have been given to whom (`crate::sim::job::Workplaces`).
    /// Empty in a save from before there were jobs, which is nobody employed.
    #[prost(message, repeated, tag = "11")]
    pub workplaces: Vec<Workplace>,
    /// What every scrap bin holds (`crate::sim::bins::Bins`). Empty in a
    /// save from before there were bins, which is a map with none.
    #[prost(message, repeated, tag = "12")]
    pub bins: Vec<Bin>,
    /// The dead counted as they went (`crate::sim::statistics::Tally`).
    /// Missing in a save from before it, which is nobody counted.
    #[prost(message, optional, tag = "13")]
    pub tally: Option<Deaths>,
    /// Every crop in the fields (`crate::sim::fields::Fields`), by cell.
    /// Empty in a save from before there were farmers, which is nothing sown.
    #[prost(message, repeated, tag = "14")]
    pub crops: Vec<Crop>,
    /// Damaged distribution boxes, packed x, y. Older saves have none.
    #[prost(sint32, repeated, tag = "15")]
    pub boxes_damaged: Vec<i32>,
    /// The playzones' computers an administrator was running as the tick
    /// ended (`crate::sim::playzone::Playzones`). Older saves have none,
    /// which is every playzone unattended until the next tick.
    #[prost(message, repeated, tag = "16")]
    pub playzones_run: Vec<Cell>,
    #[prost(message, repeated, tag = "17")]
    pub hospital: Vec<HospitalBed>,
    /// The bites dealt as the tick ended, for whoever was bitten to feel
    /// next tick (`crate::sim::wounds::Wounds`). Older saves have none,
    /// which is nobody bitten.
    #[prost(message, repeated, tag = "18")]
    pub wounds: Vec<Wound>,
    /// Walkable flower beds; empty in older saves.
    #[prost(message, repeated, tag = "19")]
    pub flowers: Vec<Flower>,
    /// The rules relationships are run by, by name
    /// (`crate::sim::relationships::Rules`). A rule not named — every one, in
    /// a save from before there were relationships — is its default.
    #[prost(message, repeated, tag = "20")]
    pub relationship_rules: Vec<RelationshipRule>,
}

/// One rule relationships are run by, under its name in `relationships.yaml`.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RelationshipRule {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(float, tag = "2")]
    pub value: f32,
}

/// One bite (`crate::sim::wounds::Wound`).
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Wound {
    #[prost(fixed64, tag = "1")]
    pub target: u64,
    #[prost(fixed64, tag = "2")]
    pub by: u64,
    /// Where the biter stood.
    #[prost(message, optional, tag = "3")]
    pub from: Option<Cell>,
    #[prost(uint32, tag = "4")]
    pub damage: u32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Crop {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    /// The grain bin of the farm it was sown for.
    #[prost(message, optional, tag = "2")]
    pub farm: Option<Cell>,
    /// When it was sown, world seconds since the world began.
    #[prost(double, tag = "3")]
    pub sown: f64,
    /// How well it has been kept, in percent.
    #[prost(uint32, tag = "4")]
    pub quality: u32,
    /// The day it was last tended, or sown.
    #[prost(uint32, tag = "5")]
    pub tended: u32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Deaths {
    #[prost(uint32, tag = "1")]
    pub died: u32,
    #[prost(uint32, tag = "2")]
    pub cremated: u32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Fridge {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    #[prost(bool, tag = "2")]
    pub open: bool,
    #[prost(float, tag = "3")]
    pub temperature: f32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Door {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    #[prost(float, tag = "2")]
    pub openness: f32,
    #[prost(float, tag = "3")]
    pub held: f32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Bin {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    /// Pieces of scrap metal in it.
    #[prost(uint32, tag = "2")]
    pub stored: u32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Workplace {
    #[prost(message, optional, tag = "1")]
    pub desk: Option<Cell>,
    #[prost(fixed64, tag = "2")]
    pub worker: u64,
    /// The hour the shift it is given for starts: a guard post is three
    /// workplaces, one a watch. Absent in a save from before the watches,
    /// which is office hours (9).
    #[prost(uint32, optional, tag = "3")]
    pub start: Option<u32>,
    /// The days the shift is worked on, bit 0 Monday: a playzone's computer
    /// is two day shifts and two evening shifts, a pair of each starting at
    /// the same hour on alternate days. Absent in a save from before there
    /// were two shifts of the same hour, which is the days of the shift its
    /// worker keeps (`Workplaces::settle`).
    #[prost(uint32, optional, tag = "4")]
    pub days: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Home {
    #[prost(message, optional, tag = "1")]
    pub bed: Option<Cell>,
    #[prost(fixed64, tag = "2")]
    pub owner: u64,
}

/// The arena's bookkeeping: which slots exist, holes included, the next
/// arrival's serial, and the order the holes will be filled in. Slot order
/// is the order everything is done in, so a world that came back with its
/// entities in other slots would not be the same world.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Arena {
    /// Slots, holes included.
    #[prost(uint32, tag = "1")]
    pub capacity: u32,
    #[prost(uint64, tag = "2")]
    pub next_serial: u64,
    /// The free list, bottom of the stack first.
    #[prost(uint32, repeated, tag = "3")]
    pub free: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct HospitalBed {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    #[prost(fixed64, tag = "2")]
    pub patient: u64,
    #[prost(uint32, tag = "3")]
    pub care: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Flower {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    /// Stable content name, independent of the runtime art ordering.
    #[prost(string, tag = "2")]
    pub variety: String,
    #[prost(double, tag = "3")]
    pub sown: f64,
}
