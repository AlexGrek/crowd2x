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
pub struct Workplace {
    #[prost(message, optional, tag = "1")]
    pub desk: Option<Cell>,
    #[prost(fixed64, tag = "2")]
    pub worker: u64,
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
