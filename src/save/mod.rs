//! Saved games: a `.savegame` file, and how a world goes into one and comes
//! back out.
//!
//! **Plain Rust, no `bevy::`**, like `sim/` and `map/`: a save is made from a
//! [`GameState`] and gives one back, and the tests run both directions with no
//! `App`. The game screen (`crate::game::saving`) and the Load screen
//! (`crate::saves`) are the Bevy side.
//!
//! # The file
//!
//! A `.savegame` is a **zip of Protobuf files** ([`archive`]), one per part of
//! the world, so a part can grow — or a new part arrive — without the others
//! changing:
//!
//! ```text
//! manifest.pb        what it is: name, when, which map, how far in   pb::Manifest
//! map.pb             the site, as a map file would hold it          pb::SiteMap
//! world.pb           the clock, the ids, switches, fridges, doors,  pb::World
//!                    beds, the arena's layout
//! chunks/NNNN.pb     the endless world: ground and salvage,         pb::Chunks
//!                    CHUNKS_PER_FILE chunks a file
//! entities/NNNN.pb   the crowd, SLOTS_PER_FILE slots a file          pb::EntityShard
//! view.pb            where the game screen was looking (optional)   pb::View
//! ```
//!
//! The schema is [`pb`], and its docs are the rules it evolves by.
//!
//! # Exactly the same world
//!
//! A world saved and loaded carries on **exactly** as it would have, to the
//! bit, tick after tick — `sim::saving` says what that takes, and this
//! module's tests run a saved world and its restored copy side by side to
//! hold it to that.
//!
//! # Fast
//!
//! Making a save is two halves. **Encoding** reads the world, so it happens
//! where the world is, between ticks — on the game's main thread — but the
//! crowd and the chunks are cut into files that are encoded in parallel
//! ([`snapshot`]). **Compressing and writing** need nothing of the world, so
//! they happen on a thread of their own, the entries compressed in parallel
//! at Deflate's fastest level ([`archive`]). The game screen only ever waits
//! for the first half.

pub mod archive;
mod error;
pub mod pb;
pub mod store;

pub use error::{invalid, InFile, Invalid, SaveError};
pub use store::{SaveEntry, SaveStore};

use prost::Message;
use rayon::prelude::*;

use crate::map::Map;
use crate::sim::GameState;
use crate::sim::RestoredEntity;

/// The format this build writes, and the newest it reads. Protobuf takes
/// care of fields and files added; this is for the day something cannot be
/// added and has to change, when an older save needs converting rather than
/// reading.
pub const FORMAT: u32 = 1;

/// Slots of the entity arena per `entities/` file. Small enough that a crowd
/// of thousands is a few files encoded and compressed side by side; big
/// enough that a small one is one.
pub const SLOTS_PER_FILE: usize = 1024;

/// Chunks per `chunks/` file, for the same reason.
pub const CHUNKS_PER_FILE: usize = 256;

/// How many quick saves are kept: a new one past this many takes the place
/// of the oldest ([`SaveStore::prune_quick`]).
pub const QUICK_SAVES_KEPT: usize = 4;

pub const MANIFEST: &str = "manifest.pb";
pub const MAP: &str = "map.pb";
pub const WORLD: &str = "world.pb";
pub const VIEW: &str = "view.pb";
pub const ECONOMY: &str = "economy.pb";
const ENTITIES: &str = "entities/";
const CHUNKS: &str = "chunks/";

/// What a save says about itself, beyond the world it holds.
#[derive(Clone, Debug, Default)]
pub struct SaveMeta {
    /// What the player called it.
    pub name: String,
    /// One of the few quick saves kept.
    pub quick: bool,
    /// The map the game was playing.
    pub map_title: String,
    /// When, in milliseconds since the Unix epoch.
    pub created_unix_ms: i64,
    /// When, as the player's clock reads it.
    pub created_local: String,
}

/// A save's files, encoded and not yet zipped: name and bytes, in the order
/// they go into the archive.
#[derive(Clone, Debug, Default)]
pub struct SaveFiles {
    pub files: Vec<(String, Vec<u8>)>,
}

impl SaveFiles {
    /// The bytes of the file called `name`.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.iter().find(|(file, _)| file == name).map(|(_, bytes)| bytes.as_slice())
    }

    /// Every file whose name starts with `prefix`, in name order.
    fn under(&self, prefix: &str) -> Vec<(&str, &[u8])> {
        let mut found: Vec<(&str, &[u8])> = self
            .files
            .iter()
            .filter(|(file, _)| file.starts_with(prefix))
            .map(|(file, bytes)| (file.as_str(), bytes.as_slice()))
            .collect();
        found.sort_unstable_by_key(|&(file, _)| file);
        found
    }

    /// The uncompressed size of every file together, in bytes.
    pub fn raw_size(&self) -> usize {
        self.files.iter().map(|(_, bytes)| bytes.len()).sum()
    }
}

/// The name of a sharded file: `entities/0003.pb`.
fn shard(dir: &str, index: usize) -> String {
    format!("{dir}{index:04}.pb")
}

/// Encode a world, and what is said about it, into a save's files. Read-only;
/// the crowd and the chunks are encoded in parallel.
pub fn snapshot(state: &GameState, meta: &SaveMeta, view: Option<&pb::View>) -> SaveFiles {
    let manifest = pb::Manifest {
        format: FORMAT,
        game_version: env!("CARGO_PKG_VERSION").to_string(),
        name: meta.name.clone(),
        quick: meta.quick,
        created_unix_ms: meta.created_unix_ms,
        created_local: meta.created_local.clone(),
        map_title: meta.map_title.clone(),
        tick: state.tick(),
        elapsed: state.elapsed(),
        population: state.len() as u64,
    };
    let entities = state.save_entities(SLOTS_PER_FILE, |shard| shard.encode_to_vec());
    let chunks: Vec<Vec<u8>> =
        state.map.save_chunks(CHUNKS_PER_FILE).into_par_iter().map(|chunks| chunks.encode_to_vec()).collect();

    let mut files = vec![
        (MANIFEST.to_string(), manifest.encode_to_vec()),
        (MAP.to_string(), state.map.save_site().encode_to_vec()),
        (WORLD.to_string(), state.save_world().encode_to_vec()),
        (ECONOMY.to_string(), state.economy().save().encode_to_vec()),
    ];
    files.extend(chunks.into_iter().enumerate().map(|(index, bytes)| (shard(CHUNKS, index), bytes)));
    files.extend(entities.into_iter().enumerate().map(|(index, bytes)| (shard(ENTITIES, index), bytes)));
    if let Some(view) = view {
        files.push((VIEW.to_string(), view.encode_to_vec()));
    }
    SaveFiles { files }
}

/// A game read back from a save.
pub struct LoadedGame {
    pub state: GameState,
    pub manifest: pb::Manifest,
    /// Where the game screen was looking; `None` for a save without it.
    pub view: Option<pb::View>,
}

/// Decode `file` as a `T`, naming the file if it is not one.
fn decode<T: Message + Default>(file: &str, bytes: &[u8]) -> Result<T, SaveError> {
    T::decode(bytes).map_err(|error| SaveError::Decode(file.to_string(), error))
}

/// Decode the file every save has.
fn required<T: Message + Default>(files: &SaveFiles, file: &str) -> Result<T, SaveError> {
    decode(file, files.get(file).ok_or_else(|| SaveError::Missing(file.to_string()))?)
}

/// What a save says it is, and whether this build can read it.
pub fn manifest(files: &SaveFiles) -> Result<pb::Manifest, SaveError> {
    let manifest: pb::Manifest = required(files, MANIFEST)?;
    check_format(&manifest)?;
    Ok(manifest)
}

/// Refuse a save from a newer format than this build reads.
pub fn check_format(manifest: &pb::Manifest) -> Result<(), SaveError> {
    if manifest.format > FORMAT || manifest.format == 0 {
        return Err(SaveError::NewerFormat { found: manifest.format, supported: FORMAT });
    }
    Ok(())
}

/// The game a save's files hold, built back exactly: the site, the chunks
/// round it, the world's own state, and the crowd — each shard of it read
/// in parallel. Everything is checked on the way in; anything this build
/// will not accept is an error naming the file and what.
pub fn restore(files: &SaveFiles) -> Result<LoadedGame, SaveError> {
    let manifest = manifest(files)?;
    let mut map = Map::restore_site(&required(files, MAP)?).in_file(MAP)?;
    let chunk_files = files
        .under(CHUNKS)
        .into_par_iter()
        .map(|(file, bytes)| decode::<pb::Chunks>(file, bytes))
        .collect::<Result<Vec<_>, _>>()?;
    map.restore_chunks(&chunk_files).in_file(CHUNKS)?;
    let revision = map.navigation_revision();

    let world: pb::World = required(files, WORLD)?;
    let entities = files
        .under(ENTITIES)
        .into_par_iter()
        .map(|(file, bytes)| {
            let shard: pb::EntityShard = decode(file, bytes)?;
            shard
                .slots
                .iter()
                .map(|record| RestoredEntity::restore(record, revision))
                .collect::<Result<Vec<_>, _>>()
                .in_file(file)
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    let mut state = GameState::restore(map, &world, entities).in_file(WORLD)?;
    let economy = files.get(ECONOMY).map(|bytes| decode::<pb::Economy>(ECONOMY, bytes)).transpose()?;
    state.restore_economy(economy.as_ref()).in_file(ECONOMY)?;
    let view = files.get(VIEW).map(|bytes| decode(VIEW, bytes)).transpose()?;
    Ok(LoadedGame { state, manifest, view })
}

#[cfg(test)]
mod tests;
