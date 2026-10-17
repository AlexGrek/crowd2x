//! The zip a save is: its files in, its files out.
//!
//! **Standard Deflate**, so any zip tool opens a `.savegame` and shows its
//! files — which is a debugging tool in its own right — at the fastest level,
//! because a save is written while somebody is playing and Protobuf has
//! already packed the numbers tight. Each file is compressed on its own
//! thread ([`zip::write::ZipFileBuilder`]) and then appended to the archive
//! as it is, so a crowd cut into shards compresses side by side.
//!
//! Compression gets a pool of its own, half the machine, rather than the
//! global one: the simulation's rounds run on the global pool, and a save
//! being written while the world carries on must not take the threads the
//! next tick is waiting for.

use std::io::{Cursor, Read, Seek, Write};
use std::sync::OnceLock;

use rayon::prelude::*;
use zip::write::{SimpleFileOptions, ZipFileBuilder};
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::{pb, SaveError, SaveFiles, MANIFEST};

/// Deflate's fastest level.
const LEVEL: i64 = 1;

/// The largest file a save may hold once inflated. Far past anything real;
/// there so a broken or hostile archive cannot ask for all the memory there
/// is.
const LARGEST_FILE: u64 = 1 << 32;

fn pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).max(1));
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("save {index}"))
            .build()
            .expect("a thread pool for saving")
    })
}

/// When a file in the archive says it was written. Zip keeps local time with
/// no zone, to two seconds.
#[derive(Clone, Copy, Debug)]
pub struct Stamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl SaveFiles {
    /// The zip of these files, in their order, each compressed in parallel.
    /// `stamp` is what every entry says it was modified at; `None` for the
    /// zip format's earliest date, which makes the bytes a function of the
    /// files alone.
    pub fn to_zip(&self, stamp: Option<Stamp>) -> Result<Vec<u8>, SaveError> {
        let mut options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(LEVEL));
        if let Some(Stamp { year, month, day, hour, minute, second }) = stamp
            && let Ok(time) = zip::DateTime::from_date_and_time(year, month, day, hour, minute, second)
        {
            options = options.last_modified_time(time);
        }
        let prepared = pool().install(|| {
            self.files
                .par_iter()
                .map(|(name, bytes)| {
                    let mut builder = ZipFileBuilder::new(name, options)?;
                    builder.write_all(bytes).map_err(zip::result::ZipError::Io)?;
                    builder.finish()
                })
                .collect::<Result<Vec<_>, _>>()
        })?;
        let compressed: usize = self.raw_size() / 2;
        let mut zip = ZipWriter::new(Cursor::new(Vec::with_capacity(compressed)));
        for file in prepared {
            zip.add_prepared_file(file)?;
        }
        Ok(zip.finish()?.into_inner())
    }

    /// Every file in a save's zip, inflated in parallel. Directories and
    /// anything with a name that would climb out of the archive are skipped;
    /// a file is read whatever its name, so a file a later build added
    /// comes along and is ignored by whatever does not know it.
    pub fn from_zip(bytes: &[u8]) -> Result<SaveFiles, SaveError> {
        let archive = ZipArchive::new(Cursor::new(bytes))?;
        let files = (0..archive.len())
            .into_par_iter()
            .map(|index| {
                let mut archive = archive.clone();
                let mut file = archive.by_index(index)?;
                if file.is_dir() || file.enclosed_name().is_none() {
                    return Ok(None);
                }
                let name = file.name()?.to_string();
                read_all(&mut file, &name).map(|bytes| Some((name, bytes)))
            })
            .collect::<Result<Vec<_>, SaveError>>()?;
        Ok(SaveFiles { files: files.into_iter().flatten().collect() })
    }
}

/// Inflate one file, refusing one that claims to be enormous.
fn read_all(file: &mut impl Read, name: &str) -> Result<Vec<u8>, SaveError> {
    let mut bytes = Vec::new();
    file.take(LARGEST_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| SaveError::Zip(format!("{name}: {error}")))?;
    if bytes.len() as u64 > LARGEST_FILE {
        return Err(SaveError::Zip(format!("{name} is larger than any save writes")));
    }
    Ok(bytes)
}

/// Only the manifest of a save, read from its zip without reading anything
/// else — what the Load screen lists.
pub fn read_manifest(zip: impl Read + Seek) -> Result<pb::Manifest, SaveError> {
    use prost::Message;
    let mut archive = ZipArchive::new(zip)?;
    let mut file = archive.by_name(MANIFEST).map_err(|_| SaveError::Missing(MANIFEST.to_string()))?;
    let bytes = read_all(&mut file, MANIFEST)?;
    pb::Manifest::decode(bytes.as_slice()).map_err(|error| SaveError::Decode(MANIFEST.to_string(), error))
}
