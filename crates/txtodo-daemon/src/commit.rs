//! `Commit`/`CommitTail`: what `FileActor::commit` (`actor.rs`) lands in one store transaction.
//! Split out purely to keep `actor.rs` within its file budget — every user still reaches these as
//! `crate::actor::{Commit, CommitTail}` via that module's re-export.

use crate::state::DocState;
use txtodo_model::{Op, TaskId};
use txtodo_store::ReviewRow;

/// One persisted change, ready to commit.
pub(crate) struct Commit {
    pub(crate) ops: Vec<Op>,
    pub(crate) next: DocState,
    pub(crate) bytes: Vec<u8>,
    pub(crate) write: bool,
    pub(crate) snapshot: bool,
    pub(crate) tail: CommitTail,
}

/// What a commit does besides landing ops and bytes (plan M4 sync paths).
pub(crate) struct CommitTail {
    /// needs_review flags to raise with this change.
    pub(crate) review: Vec<ReviewRow>,
    /// Feed the ops to the mirror afterwards (false when the mirror already holds them: import).
    pub(crate) flush: bool,
    /// Clear this flag in the store transaction (a resolution).
    pub(crate) clear: Option<(TaskId, u64)>,
    /// Store the mirror snapshot in the store transaction (an import).
    pub(crate) persist_mirror: bool,
}

impl Default for CommitTail {
    fn default() -> CommitTail {
        CommitTail {
            review: Vec::new(),
            flush: true,
            clear: None,
            persist_mirror: false,
        }
    }
}
