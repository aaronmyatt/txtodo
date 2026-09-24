//! Identifier newtypes. Distinct types over raw ints and strings (constitution §3: invalid states
//! unrepresentable). The ULID-backed ids store the raw 128 bits so `serde` derives work without
//! touching `txtodo-core` (which has no serde); `ulid()` gives the core view back.

use core::fmt;
use serde::{Deserialize, Serialize};
use txtodo_core::Ulid;

/// The device that produced an op (design §4.1). A ULID minted once per install, kept in `meta`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeviceId(u128);

/// A task's identity: the ULID in its `id:` tag (ADR 0009).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TaskId(u128);

/// One op's identity; unique across devices (ULID).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OpId(u128);

/// An agent token's identity (M6); a ULID so it sorts by creation time in `txtodo token list`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TokenId(u128);

impl DeviceId {
    /// Wraps a ULID.
    pub const fn new(ulid: Ulid) -> DeviceId {
        DeviceId(ulid.to_u128())
    }
    /// The ULID view.
    pub const fn ulid(self) -> Ulid {
        Ulid::from_u128(self.0)
    }
}

impl TaskId {
    /// Wraps a ULID.
    pub const fn new(ulid: Ulid) -> TaskId {
        TaskId(ulid.to_u128())
    }
    /// The ULID view.
    pub const fn ulid(self) -> Ulid {
        Ulid::from_u128(self.0)
    }
}

impl OpId {
    /// Wraps a ULID.
    pub const fn new(ulid: Ulid) -> OpId {
        OpId(ulid.to_u128())
    }
    /// The ULID view.
    pub const fn ulid(self) -> Ulid {
        Ulid::from_u128(self.0)
    }
}

impl TokenId {
    /// Wraps a ULID.
    pub const fn new(ulid: Ulid) -> TokenId {
        TokenId(ulid.to_u128())
    }
    /// The ULID view.
    pub const fn ulid(self) -> Ulid {
        Ulid::from_u128(self.0)
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ulid())
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ulid())
    }
}

/// Longest accepted workspace-relative path, in bytes. Deep `ref:` nesting stays far below this.
pub const FILE_PATH_MAX_BYTES: usize = 1024;

/// A synced document's path relative to the workspace root, with `/` separators. Parsed once at the
/// boundary (gRPC, walker); interior code trusts it.
///
/// Decoding is a boundary too: a peer's postcard `Op` names its own `file`, so `Deserialize` runs
/// `new` and a `../` from a paired device is a decode error, not a write outside the root (task
/// security-m6-review, F6). Serializing is unchanged, so signed ops' bytes do not move.
/// <https://serde.rs/container-attrs.html#try_from>
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct FilePath(String);

/// Why a path was rejected. The message names the offending value (constitution §3 errors).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilePathError {
    /// Empty string.
    Empty,
    /// Longer than [`FILE_PATH_MAX_BYTES`].
    TooLong(usize),
    /// Starts with `/` or contains a backslash, a `..` segment or an empty segment.
    Malformed(String),
}

impl fmt::Display for FilePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilePathError::Empty => write!(f, "file path is empty"),
            FilePathError::TooLong(n) => {
                write!(f, "file path is {n} bytes, max {FILE_PATH_MAX_BYTES}")
            }
            FilePathError::Malformed(p) => write!(f, "file path {p:?} is not workspace-relative"),
        }
    }
}

impl std::error::Error for FilePathError {}

impl FilePath {
    /// Validates external input: relative, forward slashes, no `.`/`..`/empty segments.
    pub fn new(path: &str) -> Result<FilePath, FilePathError> {
        if path.is_empty() {
            return Err(FilePathError::Empty);
        }
        if path.len() > FILE_PATH_MAX_BYTES {
            return Err(FilePathError::TooLong(path.len()));
        }
        let bad_start = path.starts_with('/') || path.contains('\\');
        let bad_segment = path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..");
        if bad_start || bad_segment {
            return Err(FilePathError::Malformed(path.to_owned()));
        }
        debug_assert!(!path.ends_with('/'), "empty last segment is rejected above");
        Ok(FilePath(path.to_owned()))
    }
    /// The path text.
    pub fn as_str(&self) -> &str {
        debug_assert!(!self.0.is_empty(), "constructed through new()");
        &self.0
    }
}

impl TryFrom<String> for FilePath {
    type Error = FilePathError;

    fn try_from(path: String) -> Result<FilePath, FilePathError> {
        FilePath::new(&path)
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_path_accepts_nested_and_rejects_escapes() {
        assert!(FilePath::new("todo.txt").is_ok());
        assert!(FilePath::new("q4-roadmap/sync/notes.md").is_ok());
        assert_eq!(FilePath::new(""), Err(FilePathError::Empty));
        for bad in ["/todo.txt", "../todo.txt", "a//b", "a/./b", "a\\b", "a/"] {
            assert!(
                matches!(FilePath::new(bad), Err(FilePathError::Malformed(_))),
                "{bad}"
            );
        }
        let long = "a".repeat(FILE_PATH_MAX_BYTES + 1);
        assert_eq!(
            FilePath::new(&long),
            Err(FilePathError::TooLong(FILE_PATH_MAX_BYTES + 1))
        );
    }

    #[test]
    fn file_path_decode_runs_the_same_checks_as_new() {
        // postcard writes a newtype String as the bare string, so this is exactly what a peer
        // who skipped `FilePath::new` on their side puts on the wire.
        // <https://postcard.jamesmunns.com/wire-format#16---newtype_struct>
        let good = postcard::to_allocvec(&FilePath::new("q4/todo.txt").unwrap()).unwrap();
        let back: FilePath = postcard::from_bytes(&good).unwrap();
        assert_eq!(back.as_str(), "q4/todo.txt");
        let long = "a".repeat(FILE_PATH_MAX_BYTES + 1);
        for bad in ["", "../todo.txt", "a/../../b", "/etc/passwd", "a\\b", &long] {
            let bytes = postcard::to_allocvec(&bad.to_owned()).unwrap();
            assert!(
                postcard::from_bytes::<FilePath>(&bytes).is_err(),
                "{bad:.40} decoded"
            );
        }
    }

    #[test]
    fn ids_round_trip_through_ulid_and_postcard() {
        let ulid = Ulid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        let id = TaskId::new(ulid);
        assert_eq!(id.ulid(), ulid);
        let bytes = postcard::to_allocvec(&id).unwrap();
        let back: TaskId = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, id);
        assert_eq!(DeviceId::new(ulid).to_string(), ulid.to_string());
    }
}
