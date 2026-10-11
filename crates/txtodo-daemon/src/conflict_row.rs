//! `FileConflicts`: what `ActorMsg::Conflicts` answers, split out to keep `handle.rs` within its
//! file budget and reached as `crate::handle::FileConflicts` through that module's re-export. The
//! needs_review flags, `ConflictRow` and `Resolution` it also carried went with the Loro mirror
//! (ADR 0038, conflict review A): nothing raises a flag any more.

use crate::duplicates::DuplicateGroup;

/// One document's conflicts: its exact-duplicate lines derived from the state right now (ADR
/// 0032).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileConflicts {
    /// Duplicate groups (`duplicates::duplicate_groups`).
    pub duplicates: Vec<DuplicateGroup>,
}
