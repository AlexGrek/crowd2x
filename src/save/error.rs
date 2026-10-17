//! What can go wrong reading a save, said precisely enough to act on.
//!
//! Loading refuses rather than repairs, as a map file does
//! (`crate::map::format`): a world that half-loaded — a unit whose task
//! points at a fridge that is not there, a uid in two places — would go on
//! to do things nobody can explain. So every check on the way in is an
//! error, and the error says which file and what.

use std::fmt;
use std::path::PathBuf;

/// Something in a save that this build will not accept: a name it does not
/// know, a number out of range, a part missing. Raised by whatever restores
/// that part, and put in a file by [`SaveError::Invalid`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid(pub String);

impl Invalid {
    pub fn new(what: impl Into<String>) -> Invalid {
        Invalid(what.into())
    }

    /// The same problem, said to be inside `part`: `brain: memory: ...`.
    pub fn within(self, part: impl fmt::Display) -> Invalid {
        Invalid(format!("{part}: {}", self.0))
    }
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Shorthand for an [`Invalid`] result.
pub fn invalid<T>(what: impl Into<String>) -> Result<T, Invalid> {
    Err(Invalid::new(what))
}

/// Why a save could not be written, listed, read or removed.
#[derive(Debug)]
pub enum SaveError {
    /// The file system said no.
    Io(PathBuf, std::io::Error),
    /// Not a zip, or a broken one.
    Zip(String),
    /// A file every save has is not in this one.
    Missing(String),
    /// A file is not the message it should be.
    Decode(String, prost::DecodeError),
    /// Made by a build that writes a newer format than this one reads.
    NewerFormat { found: u32, supported: u32 },
    /// A file decoded, and says something this build will not accept.
    Invalid { file: String, what: Invalid },
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Io(path, error) => write!(f, "{}: {error}", path.display()),
            SaveError::Zip(what) => write!(f, "not a readable save: {what}"),
            SaveError::Missing(file) => write!(f, "the save has no {file}"),
            SaveError::Decode(file, error) => write!(f, "{file} is broken: {error}"),
            SaveError::NewerFormat { found, supported } => write!(
                f,
                "made by a newer crowd2x (save format {found}; this one reads up to {supported})"
            ),
            SaveError::Invalid { file, what } => write!(f, "{file}: {what}"),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SaveError::Io(_, error) => Some(error),
            SaveError::Decode(_, error) => Some(error),
            _ => None,
        }
    }
}

impl From<zip::result::ZipError> for SaveError {
    fn from(error: zip::result::ZipError) -> SaveError {
        SaveError::Zip(error.to_string())
    }
}

/// Put an [`Invalid`] in the file it was found in.
pub trait InFile<T> {
    fn in_file(self, file: &str) -> Result<T, SaveError>;
}

impl<T> InFile<T> for Result<T, Invalid> {
    fn in_file(self, file: &str) -> Result<T, SaveError> {
        self.map_err(|what| SaveError::Invalid { file: file.to_string(), what })
    }
}
