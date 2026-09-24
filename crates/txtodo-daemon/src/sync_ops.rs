//! Committing a peer's already-signed ops from the LAN sync protocol (`lan.rs`, plan M4
//! `sync-lan-transport`). Same `impl FileActor`; split out for the file budget, same pattern as
//! `import.rs`/`actor_mirror.rs`.
//!
//! This is a different path from `import.rs`'s `on_import`: that one merges a peer's *Loro CRDT*
//! update and derives brand-new local ops (fresh `OpId`s, this device's own `Hlc`) to represent the
//! diff — the right shape for reconciling a live editor's external edit. The wire protocol's `Ops`
//! message instead carries the origin device's *own* `Op`s verbatim — each with its own `OpId`,
//! `Hlc` and `Principal` already fixed by whoever first wrote it — because `heads`/`origin_seq`
//! dedup (`want.rs`) only works if every peer stores exactly the same ops under exactly the same
//! identity. So this path never ticks this actor's own clock and never mints anything: it applies
//! the batch exactly as it arrived and commits.

use crate::actor::{Commit, CommitTail, FileActor};
use crate::handle::{ActorError, ActorHandle, ActorMsg};
use crate::state::{DocState, StateError};
use txtodo_model::{Op, OpKind, TaskId};

impl ActorHandle {
    /// Sends a peer's already-signed LAN sync ops (`lan.rs`) to this document's actor. `ops` must
    /// already be filtered to this handle's own `path` — moved out of `handle.rs` purely to keep
    /// that file within its line budget, same pattern as `refdir.rs`/`notes_lookup.rs`.
    pub(crate) async fn sync_import_ops(&self, ops: Vec<Op>) -> Result<(), ActorError> {
        debug_assert!(ops.iter().all(|o| &o.file == self.path()));
        self.ask(|reply| ActorMsg::SyncOps { ops, reply }).await?
    }
}

impl FileActor {
    /// Applies `ops` (already filtered to this actor's `path` by the caller — one commit is one
    /// document, same invariant `commit_change_with` asserts) in the order they arrived, and
    /// commits every one of them to the log, applied or not (task `sync-poison-op`, 2026-09-25).
    /// An op that does not fit this document used to refuse the whole batch, so one bad op in a
    /// peer's log blocked the workspace on every reconnect. Now it is retried once the rest of its
    /// commit (its HLC stamp: one tick per batch) has applied, the order a reconciler that
    /// anchored on a later insert needed, and skipped if it still does not fit, with a warn that
    /// names it. It stays in the log so heads stay dense and other peers still get it. Only a
    /// store or disk failure refuses the batch now.
    pub(crate) fn on_sync_ops(&mut self, ops: Vec<Op>) -> Result<(), ActorError> {
        debug_assert!(
            ops.iter().all(|o| o.file == self.cfg.path),
            "the caller routes by op.file before calling here"
        );
        if ops.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        for (op, e) in apply_leniently(&mut next, &ops) {
            log_skipped(op, &e);
        }
        let bytes = next.to_bytes();
        let write = bytes != self.projection;
        self.commit(Commit {
            ops,
            next,
            bytes,
            write,
            snapshot: false,
            tail: CommitTail {
                source: Some("sync".to_owned()),
                ..CommitTail::default()
            },
        })?;
        Ok(())
    }
}

/// Applies `ops` to `state` one commit (same HLC stamp) at a time; within a commit, an op that
/// fails is retried after the others, until a round applies nothing new. Returns the ops that
/// never applied, with why.
fn apply_leniently<'a>(state: &mut DocState, ops: &'a [Op]) -> Vec<(&'a Op, StateError)> {
    let mut skipped = Vec::new();
    for group in ops.chunk_by(|a, b| a.hlc == b.hlc) {
        let mut pending: Vec<&Op> = group.iter().collect();
        // Bounded: every round but the last applies at least one op.
        loop {
            let before = pending.len();
            let mut failed = Vec::new();
            for op in pending {
                if let Err(e) = state.apply(op) {
                    failed.push((op, e));
                }
            }
            if failed.is_empty() || failed.len() == before {
                skipped.extend(failed);
                break;
            }
            pending = failed.into_iter().map(|(op, _)| op).collect();
        }
    }
    debug_assert!(skipped.len() <= ops.len());
    skipped
}

fn log_skipped(op: &Op, e: &StateError) {
    tracing::warn!(
        file = %op.file,
        op = %op.id.ulid(),
        kind = txtodo_store::kind_tag(&op.kind),
        task = task_of(&op.kind).map(|t| t.to_string()),
        error = %e,
        "sync_op_skipped"
    );
}

/// The task an op names, when it names one.
fn task_of(kind: &OpKind) -> Option<TaskId> {
    match kind {
        OpKind::Insert { task, .. }
        | OpKind::SetField { task, .. }
        | OpKind::EditText { task, .. }
        | OpKind::Move { task, .. } => Some(*task),
        OpKind::NotesEdit { .. } | OpKind::BlankInsert { .. } | OpKind::BlankRemove { .. } => None,
    }
}
