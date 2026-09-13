//! `Resolution`/`ConflictRow`: the needs_review resolution shapes `handle.rs`'s `ActorMsg::Resolve`
//! carries. Split out purely to keep `handle.rs` within its file budget; every user still reaches
//! these as `crate::handle::{ConflictRow, Resolution}` via that module's re-export.

use txtodo_store::ReviewRow;

/// Which side a resolution keeps. Closed set; mirrors the wire enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// This device's text at flag time.
    Mine,
    /// The peer's text at flag time.
    Theirs,
    /// What is in the file now; only the flag is cleared.
    Merged,
}

/// An open flag with the line its task sits on now (0 when it left the file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    /// The stored flag.
    pub row: ReviewRow,
    /// 1-based line, 0 when the task is no longer in the file.
    pub line_number: usize,
}
