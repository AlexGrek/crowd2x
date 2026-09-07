//! Saved maps on disk: the list the map browser shows, and the file
//! operations behind its buttons.
//!
//! A store is a directory of `<name>.json` files, and the file stem *is* the
//! map's name — no index, no metadata sidecar. That means a map can be
//! renamed, copied or thrown away with the tools already on the machine, and
//! a half-written index can never disagree with what is actually there.
//!
//! The directory is a parameter rather than a constant so the tests can point
//! a store at a temporary directory. Nothing here imports `bevy`; the browser
//! wraps a [`MapStore`] in a resource.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Map, MapFormatError};

/// Longest name a map may have, in characters.
///
/// The browser draws names into a fixed column on a 320px canvas, and a file
/// name has to survive every filesystem it might be copied to.
pub const MAX_NAME: usize = 24;

const EXTENSION: &str = "json";

/// Why a map could not be listed, read, written or removed.
#[derive(Debug)]
pub enum StorageError {
    /// The name held nothing usable once the unsafe characters were dropped.
    EmptyName,
    /// The file said something [`super::format`] would not accept.
    Format(String, MapFormatError),
    Io(PathBuf, io::Error),
    NotFound(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "a map needs a name"),
            Self::Format(name, error) => write!(f, "{name}: {error}"),
            Self::Io(path, error) => write!(f, "{}: {error}", path.display()),
            Self::NotFound(name) => write!(f, "no map called {name:?}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Format(_, error) => Some(error),
            Self::Io(_, error) => Some(error),
            _ => None,
        }
    }
}

/// What survives of a name typed by a person, or `None` if that is nothing.
///
/// This is the only thing standing between a text field and the filesystem, so
/// it works by *allowing* rather than by forbidding: letters, digits, spaces,
/// dashes and underscores, and nothing else. A rule that instead banned the
/// dangerous characters would have to be right about every one of them —
/// `..`, `/`, a NUL, a leading dash, a Windows reserved name — and it only has
/// to be wrong once to write outside the maps directory.
pub fn sanitize_name(raw: &str) -> Option<String> {
    let kept: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .take(MAX_NAME)
        .collect();

    let trimmed = kept.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// `base` if it is free, else `base 2`, `base 3`, ... — the first that `taken`
/// says is not.
///
/// Split out from the store so the numbering can be tested without touching a
/// disk, and so duplicating twice in a row cannot produce the same name twice.
pub fn next_free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    // The suffix has to fit inside the length limit too, so the stem gives way.
    (2..)
        .map(|n| {
            let suffix = format!(" {n}");
            let room = MAX_NAME.saturating_sub(suffix.len());
            let stem = base.chars().take(room).collect::<String>();
            format!("{}{suffix}", stem.trim_end())
        })
        .find(|candidate| !taken(candidate))
        .expect("the counter is unbounded")
}

/// A directory of saved maps.
pub struct MapStore {
    root: PathBuf,
}

impl MapStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_of(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.{EXTENSION}"))
    }

    /// Every saved map, sorted the way the browser lists them: by name,
    /// ignoring case, so the order does not depend on the filesystem.
    ///
    /// A missing directory is an empty list, not an error — a fresh checkout
    /// has never saved a map.
    pub fn list(&self) -> Result<Vec<String>, StorageError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(StorageError::Io(self.root.clone(), error)),
        };

        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| StorageError::Io(self.root.clone(), error))?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == EXTENSION)
                && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
            {
                names.push(stem.to_string());
            }
        }
        names.sort_by_key(|name| name.to_lowercase());
        Ok(names)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path_of(name).is_file()
    }

    /// A name like `name` that no saved map is using.
    pub fn unique_name(&self, name: &str) -> String {
        next_free_name(name, |candidate| self.exists(candidate))
    }

    pub fn load(&self, name: &str) -> Result<Map, StorageError> {
        let path = self.path_of(name);
        let json = fs::read_to_string(&path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(name.to_string()),
            _ => StorageError::Io(path.clone(), error),
        })?;
        Map::from_json(&json).map_err(|error| StorageError::Format(name.to_string(), error))
    }

    pub fn save(&self, name: &str, map: &Map) -> Result<(), StorageError> {
        if sanitize_name(name).as_deref() != Some(name) {
            return Err(StorageError::EmptyName);
        }
        let json = map
            .to_json()
            .map_err(|error| StorageError::Format(name.to_string(), error))?;
        fs::create_dir_all(&self.root)
            .map_err(|error| StorageError::Io(self.root.clone(), error))?;
        let path = self.path_of(name);
        fs::write(&path, json).map_err(|error| StorageError::Io(path, error))
    }

    pub fn delete(&self, name: &str) -> Result<(), StorageError> {
        let path = self.path_of(name);
        fs::remove_file(&path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(name.to_string()),
            _ => StorageError::Io(path.clone(), error),
        })
    }

    /// Copy a map under a free name, which is returned.
    ///
    /// The bytes are copied rather than the map being loaded and written back:
    /// a duplicate of a file this build cannot parse is still a faithful
    /// duplicate, and copying cannot quietly reformat somebody's map.
    pub fn duplicate(&self, name: &str) -> Result<String, StorageError> {
        let source = self.path_of(name);
        if !source.is_file() {
            return Err(StorageError::NotFound(name.to_string()));
        }
        let copy = self.unique_name(&truncate_name(&format!("{name} copy")));
        let destination = self.path_of(&copy);
        fs::copy(&source, &destination).map_err(|error| StorageError::Io(destination, error))?;
        Ok(copy)
    }
}

fn truncate_name(name: &str) -> String {
    name.chars().take(MAX_NAME).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Point, Size, FLOOR, WALL};

    /// A directory that deletes itself, so a failed test cannot leave a store
    /// behind for the next one to find.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("crowd2x-{label}-{unique}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn store(&self) -> MapStore {
            MapStore::new(&self.0)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn a_map() -> Map {
        let mut map = Map::new(Size::new(4, 3), FLOOR);
        map.set_terrain(Point::new(1, 1), WALL);
        map
    }

    #[test]
    fn a_name_keeps_only_characters_that_are_safe_in_a_file_name() {
        assert_eq!(sanitize_name("office 2"), Some("office 2".into()));
        assert_eq!(sanitize_name("  padded  "), Some("padded".into()));
        assert_eq!(sanitize_name("a-b_c9"), Some("a-b_c9".into()));
    }

    #[test]
    fn a_name_cannot_escape_the_maps_directory() {
        // The whole point of allow-listing: none of these can reach a path.
        assert_eq!(sanitize_name("../../etc/passwd"), Some("etcpasswd".into()));
        assert_eq!(sanitize_name("/absolute"), Some("absolute".into()));
        assert_eq!(sanitize_name("nul\0byte"), Some("nulbyte".into()));
        assert_eq!(sanitize_name("..."), None);
        assert_eq!(sanitize_name("   "), None);
        assert_eq!(sanitize_name(""), None);
    }

    #[test]
    fn a_name_is_cut_to_the_length_limit() {
        let long = "x".repeat(MAX_NAME * 2);
        assert_eq!(sanitize_name(&long).unwrap().chars().count(), MAX_NAME);
    }

    #[test]
    fn a_free_name_is_numbered_up_until_it_is_free() {
        assert_eq!(next_free_name("office", |_| false), "office");
        assert_eq!(next_free_name("office", |name| name == "office"), "office 2");
        assert_eq!(
            next_free_name("office", |name| name.starts_with("office")
                && name != "office 4"),
            "office 4"
        );
    }

    #[test]
    fn numbering_a_maximum_length_name_stays_within_the_limit() {
        // Otherwise the counter would push the name past what save() accepts
        // and duplicating a long name would fail instead of numbering it.
        let long = "y".repeat(MAX_NAME);
        let numbered = next_free_name(&long, |name| name == long);
        assert!(numbered.chars().count() <= MAX_NAME, "{numbered:?}");
        assert!(numbered.ends_with(" 2"), "{numbered:?}");
    }

    #[test]
    fn a_saved_map_comes_back_the_same() {
        let dir = TempDir::new("round-trip");
        let store = dir.store();
        store.save("office", &a_map()).unwrap();

        let loaded = store.load("office").unwrap();
        assert_eq!(loaded.size(), a_map().size());
        assert_eq!(loaded.terrain(Point::new(1, 1)), Some(WALL));
        assert!(loaded.passability().is_blocked(Point::new(1, 1)));
    }

    #[test]
    fn saving_creates_the_maps_directory() {
        let dir = TempDir::new("mkdir");
        let store = MapStore::new(dir.0.join("does-not-exist-yet"));
        assert_eq!(store.list().unwrap(), Vec::<String>::new());
        store.save("first", &a_map()).unwrap();
        assert_eq!(store.list().unwrap(), ["first"]);
    }

    #[test]
    fn listing_is_sorted_and_ignores_everything_that_is_not_a_map() {
        let dir = TempDir::new("list");
        let store = dir.store();
        for name in ["Bravo", "alpha", "charlie"] {
            store.save(name, &a_map()).unwrap();
        }
        fs::write(dir.0.join("notes.txt"), "not a map").unwrap();
        fs::create_dir(dir.0.join("subdir")).unwrap();

        assert_eq!(store.list().unwrap(), ["alpha", "Bravo", "charlie"]);
    }

    #[test]
    fn a_duplicate_gets_its_own_name_and_the_same_contents() {
        let dir = TempDir::new("duplicate");
        let store = dir.store();
        store.save("office", &a_map()).unwrap();

        assert_eq!(store.duplicate("office").unwrap(), "office copy");
        // Duplicating twice must not collide with the first copy.
        assert_eq!(store.duplicate("office").unwrap(), "office copy 2");

        let copy = store.load("office copy").unwrap();
        assert_eq!(copy.terrain(Point::new(1, 1)), Some(WALL));
        assert!(store.exists("office"), "the original is still there");
    }

    #[test]
    fn deleting_removes_only_the_named_map() {
        let dir = TempDir::new("delete");
        let store = dir.store();
        store.save("keep", &a_map()).unwrap();
        store.save("drop", &a_map()).unwrap();

        store.delete("drop").unwrap();
        assert_eq!(store.list().unwrap(), ["keep"]);
    }

    #[test]
    fn operating_on_a_map_that_is_not_there_says_so() {
        let dir = TempDir::new("missing");
        let store = dir.store();
        for error in [
            store.load("ghost").unwrap_err(),
            store.delete("ghost").unwrap_err(),
            store.duplicate("ghost").unwrap_err(),
        ] {
            assert!(matches!(error, StorageError::NotFound(name) if name == "ghost"));
        }
    }

    #[test]
    fn a_corrupt_file_is_reported_against_its_name() {
        let dir = TempDir::new("corrupt");
        let store = dir.store();
        fs::write(dir.0.join("broken.json"), "{ not json").unwrap();

        let error = store.load("broken").unwrap_err();
        assert!(matches!(&error, StorageError::Format(name, _) if name == "broken"));
        // ...and it still shows up in the list, so it can be deleted.
        assert_eq!(store.list().unwrap(), ["broken"]);
    }

    #[test]
    fn a_name_that_was_never_sanitized_is_refused_by_save() {
        let dir = TempDir::new("unsafe");
        let store = dir.store();
        assert!(matches!(
            store.save("../escape", &a_map()).unwrap_err(),
            StorageError::EmptyName
        ));
        assert!(!dir.0.parent().unwrap().join("escape.json").exists());
    }
}
