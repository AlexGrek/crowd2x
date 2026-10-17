//! Saved games on disk: `.saves/`, one `{date-time}.savegame` a save.
//!
//! The file is named after the moment it was made, `2026-10-17_14-03-22`,
//! and nothing else — what the player called it is inside, in its manifest,
//! so a name can be anything at all and two saves can share one. A second
//! save in the same second gets `-2`, `-3`, ... after its time.
//!
//! Written whole or not at all ([`crate::map::write_atomic`]): a game that
//! crashes mid-save leaves the saves it had. Listed newest first, by the time
//! inside the file; one that cannot be read is listed with what is wrong with
//! it rather than hidden, since it is still a file somebody may want to
//! delete.
//!
//! Plain Rust, like the rest of `save`; the Load screen (`crate::saves`) and
//! the game screen (`crate::game::saving`) wrap a store.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};

use super::archive::{self, Stamp};
use super::{pb, SaveError};

/// Where saves live. Relative, like `maps/` and `assets/`, so it resolves
/// against the manifest under `cargo run`.
pub const SAVES_DIR: &str = ".saves";

/// Points the store somewhere else: a scripted QA run makes and deletes
/// saves, so it needs a directory that is not somebody's games.
pub const ENV_SAVES_DIR: &str = "CROWD2X_SAVES";

pub const EXTENSION: &str = "savegame";

/// The directory saves go in: [`ENV_SAVES_DIR`] if it is set, otherwise
/// [`SAVES_DIR`].
pub fn saves_dir() -> PathBuf {
    std::env::var_os(ENV_SAVES_DIR).map_or_else(|| PathBuf::from(SAVES_DIR), PathBuf::from)
}

/// One moment, every way a save needs to say it.
#[derive(Clone, Debug)]
pub struct Moment {
    /// Milliseconds since the Unix epoch, UTC: what a save is sorted by.
    pub unix_ms: i64,
    /// As the player's clock reads it, for showing: `2026-10-17 14:03`.
    pub local: String,
    /// What the file is called, before any `-2`: `2026-10-17_14-03-22`.
    pub stem: String,
    /// What the zip's entries say they were modified at.
    pub stamp: Stamp,
}

impl Moment {
    pub fn now() -> Moment {
        Moment::at(Local::now())
    }

    pub fn at<Z: TimeZone>(time: DateTime<Z>) -> Moment
    where
        Z::Offset: std::fmt::Display,
    {
        Moment {
            unix_ms: time.timestamp_millis(),
            local: time.format("%Y-%m-%d %H:%M").to_string(),
            stem: time.format("%Y-%m-%d_%H-%M-%S").to_string(),
            stamp: Stamp {
                year: time.year().clamp(1980, 2107) as u16,
                month: time.month() as u8,
                day: time.day() as u8,
                hour: time.hour() as u8,
                minute: time.minute() as u8,
                second: time.second() as u8,
            },
        }
    }
}

/// One save in the store, as the Load screen lists it.
#[derive(Clone, Debug)]
pub struct SaveEntry {
    pub path: PathBuf,
    /// The file's own name, `2026-10-17_14-03-22.savegame`.
    pub file_name: String,
    /// What it says it is — or why that could not be read, and so why it
    /// cannot be loaded.
    pub manifest: Result<pb::Manifest, String>,
}

impl SaveEntry {
    /// Whether this build can load it.
    pub fn is_loadable(&self) -> bool {
        self.manifest.is_ok()
    }

    pub fn is_quick(&self) -> bool {
        self.manifest.as_ref().is_ok_and(|manifest| manifest.quick)
    }

    /// When it was made, for sorting: the manifest's time, or nothing for
    /// a file that could not be read — which then sorts by its name.
    fn created(&self) -> Option<i64> {
        self.manifest.as_ref().ok().map(|manifest| manifest.created_unix_ms)
    }
}

/// A directory of saved games.
pub struct SaveStore {
    root: PathBuf,
}

impl SaveStore {
    pub fn new(root: impl Into<PathBuf>) -> SaveStore {
        SaveStore { root: root.into() }
    }

    /// The store in [`saves_dir`].
    pub fn default_store() -> SaveStore {
        SaveStore::new(saves_dir())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every `.savegame` in the store, newest first. A missing directory is
    /// an empty list: nobody has saved yet.
    pub fn list(&self) -> Result<Vec<SaveEntry>, SaveError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(SaveError::Io(self.root.clone(), error)),
        };
        let mut saves = Vec::new();
        for entry in entries {
            let path = entry.map_err(|error| SaveError::Io(self.root.clone(), error))?.path();
            if !path.extension().is_some_and(|extension| extension == EXTENSION) || !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()).map(str::to_string) else {
                continue;
            };
            let manifest = fs::File::open(&path)
                .map_err(|error| SaveError::Io(path.clone(), error))
                .and_then(|file| archive::read_manifest(io::BufReader::new(file)))
                .and_then(|manifest| super::check_format(&manifest).map(|()| manifest))
                .map_err(|error| error.to_string());
            saves.push(SaveEntry { path, file_name, manifest });
        }
        saves.sort_by(|a, b| b.created().cmp(&a.created()).then_with(|| b.file_name.cmp(&a.file_name)));
        Ok(saves)
    }

    /// The newest save, if there is one.
    pub fn newest(&self) -> Result<Option<SaveEntry>, SaveError> {
        Ok(self.list()?.into_iter().find(SaveEntry::is_loadable))
    }

    /// Write a save made at `moment`, under a name nothing in the store has,
    /// and say where it went.
    pub fn write(&self, zip: &[u8], moment: &Moment) -> Result<PathBuf, SaveError> {
        // Two saves finishing together must not both take the free name.
        static NAMING: Mutex<()> = Mutex::new(());
        let _naming = NAMING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        fs::create_dir_all(&self.root).map_err(|error| SaveError::Io(self.root.clone(), error))?;
        let path = (1..)
            .map(|n| match n {
                1 => self.root.join(format!("{}.{EXTENSION}", moment.stem)),
                n => self.root.join(format!("{}-{n}.{EXTENSION}", moment.stem)),
            })
            .find(|path| !path.exists())
            .expect("some name is free");
        crate::map::write_atomic(&path, |file| io::Write::write_all(file, zip))
            .map_err(|error| SaveError::Io(path.clone(), error))?;
        Ok(path)
    }

    pub fn delete(&self, path: &Path) -> Result<(), SaveError> {
        fs::remove_file(path).map_err(|error| SaveError::Io(path.to_path_buf(), error))
    }

    /// Delete every quick save but the newest `keep`, and say how many went.
    /// Quick saves are told by their manifest, never by name: a save the
    /// player called "quick save" is theirs to delete.
    ///
    /// Two quick saves finishing together may both go to delete the same
    /// oldest one; whichever comes second finds it gone, which is what it
    /// wanted.
    pub fn prune_quick(&self, keep: usize) -> Result<usize, SaveError> {
        let stale: Vec<SaveEntry> = self.list()?.into_iter().filter(SaveEntry::is_quick).skip(keep).collect();
        for save in &stale {
            match fs::remove_file(&save.path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    return Err(SaveError::Io(save.path.clone(), error));
                }
                _ => {}
            }
        }
        Ok(stale.len())
    }
}
