//! `StateError`, why an op or a file does not fit a `DocState` (`state.rs`). Split out of
//! `state.rs` for its line budget; `state.rs` re-exports it.

use crate::state::MAX_LINES_PER_FILE;
use crate::textedit::TextEditError;
use std::fmt;
use txtodo_model::TaskId;

/// Why an op or a file could not be applied — a daemon bug or a stale client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    /// A task line has no resolved id, tag or fingerprint match (line index).
    MissingId(usize),
    /// A line is not valid UTF-8 (line index).
    Opaque(usize),
    /// The op names a task this document does not hold.
    UnknownTask(TaskId),
    /// `Insert` line text does not carry the op's task id.
    IdMismatch(TaskId),
    /// A text edit did not fit the description.
    Text(TaskId, TextEditError),
    /// `BlankRemove` found no blank at that position.
    NoBlank(Option<TaskId>),
    /// The op kind is not handled in this document model (notes, undelete-via-`SetField`).
    Unsupported(&'static str),
    /// The document would exceed `MAX_LINES_PER_FILE`.
    TooManyLines(usize),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::MissingId(i) => write!(f, "line {} has no id: tag", i + 1),
            StateError::Opaque(i) => write!(f, "line {} is not UTF-8", i + 1),
            StateError::UnknownTask(t) => write!(f, "no task {t} in this document"),
            StateError::IdMismatch(t) => write!(f, "inserted line does not carry id {t}"),
            StateError::Text(t, e) => write!(f, "task {t}: {e}"),
            StateError::NoBlank(after) => write!(f, "no blank line after {after:?}"),
            StateError::Unsupported(what) => {
                write!(f, "{what} is not supported on one device (M3)")
            }
            StateError::TooManyLines(n) => {
                write!(f, "document would have {n} lines, max {MAX_LINES_PER_FILE}")
            }
        }
    }
}

impl std::error::Error for StateError {}
