//! `Commit`/`CommitTail`: what `FileActor::commit` (`actor.rs`) lands in one store transaction.
//! Split out purely to keep `actor.rs` within its file budget — every user still reaches these as
//! `crate::actor::{Commit, CommitTail}` via that module's re-export.

use crate::actor::FileActor;
use crate::expected::Hash;
use crate::handle::Change;
use crate::state::DocState;
use txtodo_model::{Op, TaskId};
use txtodo_store::{ReviewRow, Seq, SeqRange, Stored};

/// One persisted change, ready to commit.
pub(crate) struct Commit {
    pub(crate) ops: Vec<Op>,
    pub(crate) next: DocState,
    pub(crate) bytes: Vec<u8>,
    pub(crate) write: bool,
    pub(crate) snapshot: bool,
    pub(crate) tail: CommitTail,
}

/// `CommitTail::source` for ops that came through sync: a peer's, or our own relayed back.
pub(crate) const SYNC_SOURCE: &str = "sync";

/// What a commit does besides landing ops and bytes (plan M4 sync paths).
#[derive(Default)]
pub(crate) struct CommitTail {
    /// needs_review flags to raise with this change.
    pub(crate) review: Vec<ReviewRow>,
    /// Clear this flag in the store transaction (a resolution).
    pub(crate) clear: Option<(TaskId, u64)>,
    /// Which client made the change, stamped on every op of the commit (task op-source): a
    /// client's own name, `"sync"` for ops from another device, `"external"` for a disk edit.
    /// Kept in this device's op log only, never in an op.
    pub(crate) source: Option<String>,
}

impl FileActor {
    /// Numbers the committed ops and assembles the `Change` this commit produced.
    pub(crate) fn stored_change(
        &self,
        hash: Hash,
        range: Option<SeqRange>,
        ops: Vec<Op>,
        review: Vec<ReviewRow>,
    ) -> Change {
        let first = range.map_or(0, |r| r.first.0);
        let stored: Vec<Stored> = ops
            .into_iter()
            .enumerate()
            .map(|(i, op)| Stored {
                seq: Seq(first + i as i64),
                op,
            })
            .collect();
        // After the state swap: the groups the file has now (ADR 0032). Only a Watch subscriber
        // reads the count (`watch_forward.rs`), so with none a sync burst skips a pass over every
        // line per commit; one that subscribes later lists conflicts itself.
        let groups = if self.changes.receiver_count() > 0 {
            crate::duplicates::duplicate_groups(&self.state).len()
        } else {
            0
        };
        Change {
            path: self.cfg.path.clone(),
            hash,
            ops: stored,
            review,
            duplicate_groups: u32::try_from(groups).unwrap_or(u32::MAX),
        }
    }
}
