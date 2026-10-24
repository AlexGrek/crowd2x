//! `map.pb` — the site — and `chunks/NNNN.pb`, the endless world round it.

/// The site: the cells somebody authored. Everything a map file holds, so a
/// save does not depend on the map file still being there or unchanged.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SiteMap {
    #[prost(sint32, tag = "1")]
    pub width: i32,
    #[prost(sint32, tag = "2")]
    pub height: i32,
    /// The terrain catalogue names the tiles below index into.
    #[prost(string, repeated, tag = "3")]
    pub terrain_names: Vec<String>,
    /// Bottom layer first. One, today.
    #[prost(message, repeated, tag = "4")]
    pub terrain_layers: Vec<TerrainLayer>,
    /// Only the grid layers with anything in them.
    #[prost(message, repeated, tag = "5")]
    pub grids: Vec<Grid>,
    #[prost(message, repeated, tag = "6")]
    pub objects: Vec<ObjectLayer>,
    /// The wasteland round the site; `None` for nothing round it.
    #[prost(message, optional, tag = "7")]
    pub wasteland: Option<Wasteland>,
    /// Authored supplies for a new game; current supplies are in world.pb.
    #[prost(message, repeated, tag = "8")]
    pub initial_stock: Vec<super::Bin>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TerrainLayer {
    /// Row-major, bottom row first, an index into `terrain_names` per cell.
    #[prost(uint32, repeated, tag = "1")]
    pub tiles: Vec<u32>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Grid {
    /// `crate::map::GridLayer::name`.
    #[prost(string, tag = "1")]
    pub layer: String,
    /// Row-major, one value per cell, each one the layer's alphabet accepts.
    #[prost(bytes = "vec", tag = "2")]
    pub cells: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ObjectLayer {
    /// `crate::map::ObjectLayer::name`.
    #[prost(string, tag = "1")]
    pub layer: String,
    /// The names `kind` indexes into.
    #[prost(string, repeated, tag = "2")]
    pub kinds: Vec<String>,
    /// Per object, an index into `kinds`.
    #[prost(uint32, repeated, tag = "3")]
    pub kind: Vec<u32>,
    /// Per object, its position in whole world pixels, packed `x, y`.
    #[prost(sint32, repeated, tag = "4")]
    pub at: Vec<i32>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Wasteland {
    #[prost(uint64, tag = "1")]
    pub seed: u64,
}

/// One file of the endless world: up to [`crate::save::CHUNKS_PER_FILE`]
/// chunks.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Chunks {
    /// `crate::map::wasteland::GENERATION` when written: which version of the
    /// generator the stored ground came from.
    #[prost(uint32, tag = "1")]
    pub generation: u32,
    /// The terrain catalogue names this file's tiles index into.
    #[prost(string, repeated, tag = "2")]
    pub terrain_names: Vec<String>,
    #[prost(message, repeated, tag = "3")]
    pub chunks: Vec<ChunkRecord>,
}

/// Everything the world knows about one chunk: its ground, if it is loaded,
/// and what has been taken from its heaps of scrap, whether it is loaded or
/// not.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ChunkRecord {
    #[prost(sint32, tag = "1")]
    pub x: i32,
    #[prost(sint32, tag = "2")]
    pub y: i32,
    /// The chunk's ground as it is held in memory, stored verbatim.
    #[prost(message, optional, tag = "3")]
    pub ground: Option<Ground>,
    /// Pieces taken from each heap anybody has been at.
    #[prost(message, repeated, tag = "4")]
    pub salvaged: Vec<Tally>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Ground {
    /// `CHUNK_AREA` of them, row-major, an index into the file's
    /// `terrain_names`.
    #[prost(uint32, repeated, tag = "1")]
    pub tiles: Vec<u32>,
    /// Every heap of scrap as it was found, by local index ascending.
    #[prost(message, repeated, tag = "2")]
    pub heaps: Vec<Heap>,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Heap {
    /// The cell's index in the chunk, row-major.
    #[prost(uint32, tag = "1")]
    pub index: u32,
    #[prost(uint32, tag = "2")]
    pub variant: u32,
    /// Pieces in it as found.
    #[prost(uint32, tag = "3")]
    pub pieces: u32,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Tally {
    /// The heap's cell's index in the chunk, row-major.
    #[prost(uint32, tag = "1")]
    pub index: u32,
    #[prost(uint32, tag = "2")]
    pub taken: u32,
}
