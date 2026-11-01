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
    /// Ground contamination; older worlds start clean.
    #[prost(message, repeated, tag = "21")]
    pub dirt: Vec<DirtCell>,
    /// The dog foods given as the tick ended, for the dogs to feel next tick
    /// (`crate::sim::pets::Feedings`). Older saves have none.
    #[prost(message, repeated, tag = "22")]
    pub feedings: Vec<Feeding>,
    /// The store's superfridge: the rules it is run by and the share of a
    /// unit fallen since the last whole one (`crate::sim::superfridge`).
    /// Absent in a save from before there were stores: the default rules,
    /// nothing fallen.
    #[prost(message, optional, tag = "23")]
    pub store: Option<Store>,
    /// The courier dispatch's orders not yet delivered
    /// (`crate::sim::dispatch`). Older saves have none.
    #[prost(message, repeated, tag = "24")]
    pub orders: Vec<Order>,
    /// Human arrival lottery, kept across saves, including partial groups.
    #[prost(uint64, tag = "25")]
    pub noble_draws: u64,
    #[prost(uint32, tag = "26")]
    pub noble_position: u32,
    /// Signed airborne scent; older saves start with clean air.
    #[prost(message, repeated, tag = "27")]
    pub smell: Vec<SmellCell>,
    /// The outposts the king decreed and how far each is built
    /// (`crate::sim::outposts`). Absent in an older save: none.
    #[prost(message, optional, tag = "28")]
    pub outposts: Option<Outposts>,
    #[prost(message, repeated, tag = "29")]
    pub sounds: Vec<Spoken>,
    /// The news: stories waiting for the propagandist, articles on the
    /// phones, what has been made towards the next record
    /// (`crate::sim::news`). Absent in an older save: no news yet.
    #[prost(message, optional, tag = "30")]
    pub news: Option<News>,
    /// The last week, a sample an hour (`crate::sim::history`), oldest
    /// first. Older saves have none: the graphs start from the load.
    #[prost(message, repeated, tag = "31")]
    pub history: Vec<HourSample>,
    #[prost(message, repeated, tag = "32")]
    pub decor_purchases: Vec<DecorPurchase>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct HourSample {
    /// Whole world hours since the world began.
    #[prost(uint64, tag = "1")]
    pub hour: u64,
    /// Mean mental health of the living humans.
    #[prost(float, tag = "2")]
    pub mind: f32,
    /// Mean health of the living humans.
    #[prost(float, tag = "3")]
    pub health: f32,
    /// The district's budget, in cents.
    #[prost(sint64, tag = "4")]
    pub budget: i64,
    /// The cash every living account holds, in cents.
    #[prost(sint64, tag = "5")]
    pub cash: i64,
    /// The living humans the means are of.
    #[prost(uint32, tag = "6")]
    pub humans: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Outposts {
    /// In the order decreed; only the last may be unfinished.
    #[prost(message, repeated, tag = "1")]
    pub outposts: Vec<Outpost>,
    /// The last day the king looked for a site and found none; 0, never.
    #[prost(uint32, tag = "2")]
    pub fruitless: u32,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Outpost {
    /// Where its flag stands, its middle.
    #[prost(message, optional, tag = "1")]
    pub center: Option<Cell>,
    /// Upside down, its doorway in the top row.
    #[prost(bool, tag = "2")]
    pub flipped: bool,
    /// The day it was decreed.
    #[prost(uint32, tag = "3")]
    pub decreed: u32,
    /// Its plan, tile by tile in `outposts::LAYOUT`'s order.
    #[prost(message, repeated, tag = "4")]
    pub tiles: Vec<OutpostTile>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct OutpostTile {
    #[prost(enumeration = "TileStage", tag = "1")]
    pub stage: i32,
    /// World seconds worked on its building tile, while laid.
    #[prost(float, tag = "2")]
    pub progress: f32,
    /// The builder working on it, if anybody is.
    #[prost(fixed64, optional, tag = "3")]
    pub claim: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum TileStage {
    Planned = 0,
    Laid = 1,
    Built = 2,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct SmellCell {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    #[prost(float, tag = "2")]
    pub value: f32,
}

/// The store's superfridge (`crate::sim::superfridge::Superfridge`).
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Store {
    #[prost(message, optional, tag = "1")]
    pub rules: Option<StoreRules>,
    /// The share of a unit fallen since the last whole one came out, 0 to 1.
    #[prost(double, tag = "2")]
    pub fallen: f64,
}

/// What the store is run by, as `store.yaml` says it
/// (`crate::sim::superfridge::StoreRules`).
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct StoreRules {
    /// Capacity in delivery batches, for older readers.
    #[prost(uint32, tag = "1")]
    pub capacity: u32,
    /// Percent of the capacity a meal out of a fridge takes.
    #[prost(double, tag = "2")]
    pub falls_per_meal: f64,
    #[prost(double, tag = "3")]
    pub call_at: f64,
    #[prost(double, tag = "4")]
    pub starts_at: f64,
    /// Normalized reserve units; absent in saves using whole meal units.
    #[prost(double, optional, tag = "5")]
    pub normalized_capacity: Option<f64>,
    #[prost(double, optional, tag = "6")]
    pub delivery_size: Option<f64>,
}

/// One unit of food called for (`crate::sim::dispatch::Order`).
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Order {
    /// The superfridge it is for.
    #[prost(message, optional, tag = "1")]
    pub store: Option<Cell>,
    /// Who was sent; 0 while it waits.
    #[prost(fixed64, tag = "2")]
    pub courier: u64,
    /// When it was given to the courier, world seconds since the world began.
    #[prost(double, tag = "3")]
    pub since: f64,
}

/// One dog food given to a dog (`crate::sim::pets::Feeding`).
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Feeding {
    #[prost(fixed64, tag = "1")]
    pub dog: u64,
    #[prost(fixed64, tag = "2")]
    pub by: u64,
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

/// Four normalized surface contaminants, shared by ground and bodies.
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Dirt {
    #[prost(float, tag = "1")]
    pub dirt: f32,
    #[prost(float, tag = "2")]
    pub blood: f32,
    #[prost(float, tag = "3")]
    pub wet: f32,
    #[prost(float, tag = "4")]
    pub burns: f32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct DirtCell {
    #[prost(message, optional, tag = "1")]
    pub cell: Option<Cell>,
    #[prost(message, optional, tag = "2")]
    pub value: Option<Dirt>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Spoken {
    #[prost(fixed64, tag = "1")]
    pub speaker: u64,
    #[prost(message, optional, tag = "2")]
    pub at: Option<Cell>,
    #[prost(fixed64, optional, tag = "3")]
    pub recipient: Option<u64>,
    #[prost(bool, tag = "4")]
    pub shout: bool,
    #[prost(string, tag = "5")]
    pub text: String,
}

/// `crate::sim::news::News`.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct News {
    /// Waiting for the propagandist, oldest first.
    #[prost(message, repeated, tag = "1")]
    pub stories: Vec<Story>,
    /// Published and not yet a day old, oldest first.
    #[prost(message, repeated, tag = "2")]
    pub articles: Vec<Article>,
    /// Pieces made so far, by item name; an item with none left out.
    #[prost(message, repeated, tag = "3")]
    pub made: Vec<Made>,
    /// The number the next article is published under.
    #[prost(uint32, tag = "4")]
    pub next: u32,
}

/// One story: a death (`dead` and its case: `name`, `job`, `cause`, `at`,
/// `when`) or a production record (`item`, `total`).
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Story {
    #[prost(fixed64, optional, tag = "1")]
    pub dead: Option<u64>,
    #[prost(string, optional, tag = "2")]
    pub item: Option<String>,
    #[prost(uint32, tag = "3")]
    pub total: u32,
    /// What it died of.
    #[prost(string, optional, tag = "4")]
    pub cause: Option<String>,
    /// Where it fell.
    #[prost(message, optional, tag = "5")]
    pub at: Option<Cell>,
    /// Its name.
    #[prost(string, optional, tag = "6")]
    pub name: Option<String>,
    /// Its job's catalogue number, if it had one.
    #[prost(uint32, optional, tag = "7")]
    pub job: Option<u32>,
    /// When it died, in world seconds since the world began.
    #[prost(double, optional, tag = "8")]
    pub when: Option<f64>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Article {
    #[prost(uint32, tag = "1")]
    pub id: u32,
    #[prost(message, optional, tag = "2")]
    pub story: Option<Story>,
    /// When it went up, in world seconds since the world began.
    #[prost(double, tag = "3")]
    pub published: f64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Made {
    #[prost(string, tag = "1")]
    pub item: String,
    #[prost(uint32, tag = "2")]
    pub count: u32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct DecorPurchase {
    #[prost(fixed64, tag = "1")]
    pub owner: u64,
    #[prost(uint32, tag = "2")]
    pub home: u32,
    #[prost(message, optional, tag = "3")]
    pub cell: Option<Cell>,
}
