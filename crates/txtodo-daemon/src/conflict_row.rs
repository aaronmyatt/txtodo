//! `FileConflicts`/`ConflictRow`: what `ActorMsg::Conflicts` answers. Split out purely to keep
//! `handle.rs` within its file budget; users reach these through `crate::handle`'s re-export.

use crate::duplicates::DuplicateGroup;
use txtodo_store::ReviewRow;

/// One document's conflicts: its open needs_review flags, and its exact-duplicate lines derived
/// from the state right now (ADR 0032).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileConflicts {
    /// Open flags, each with its line now.
    pub flags: Vec<ConflictRow>,
    /// Duplicate groups (`duplicates::duplicate_groups`).
    pub duplicates: Vec<DuplicateGroup>,
}

/// An open flag with the line its task sits on now (0 when it left the file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    /// The stored flag.
    pub row: ReviewRow,
    /// 1-based line, 0 when the task is no longer in the file.
    pub line_number: usize,
}
