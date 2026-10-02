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
//! the batch exactly as it arrived and commits. It does merge the batch's newest stamp into the
//! clock (the HLC receive rule), so the next local op sorts after it (task insert-order).

use crate::actor::{Commit, CommitTail, FileActor};
use crate::handle::{ActorError, ActorHandle, ActorMsg};
use crate::state::{DocState, StateError};
use crate::sync_park::apply_parking;
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
        self.observe_peer_stamps(&ops);
        let mut next = self.state.clone();
        // An op naming a task another device has not delivered yet waits for it (`sync_park.rs`).
        let mut parked = self.parked.clone();
        let parking = apply_parking(&mut next, &ops, &mut parked);
        for (op, e) in &parking.skipped {
            log_skipped(op, e);
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
        self.parked = parked;
        if parking.landed > 0 {
            // Ops from earlier batches landed in this one: the mirror only saw this batch.
            self.converge_mirror();
        }
        Ok(())
    }

    /// HLC receive rule for the batch's newest stamp (`Hlc::merge`), so this device's next op
    /// sorts after every line it now holds: a peer's line placed "later" than a local op would
    /// otherwise be skipped over by it (task insert-order, `state_order.rs`). A stamp more than
    /// the skew bound ahead is not merged, the same refusal the link `Hello` makes.
    fn observe_peer_stamps(&mut self, ops: &[Op]) {
        let Some(newest) = ops.iter().map(|op| op.hlc).max() else {
            return;
        };
        let before = self.hlc;
        if let Err(e) = self.hlc.merge(newest, self.clock.now_ms()) {
            log_stamp_not_merged(op_file(ops), &e);
        }
        debug_assert!(self.hlc >= before, "merge never goes back");
        debug_assert_eq!(self.hlc.device, self.cfg.device);
    }
}

fn op_file(ops: &[Op]) -> Option<&txtodo_model::FilePath> {
    ops.first().map(|op| &op.file)
}

/// tracing `warn!`, split out for the caller's complexity budget.
/// <https://docs.rs/tracing/latest/tracing/macro.warn.html>
fn log_stamp_not_merged(file: Option<&txtodo_model::FilePath>, e: &txtodo_model::HlcError) {
    tracing::warn!(file = file.map(|f| f.to_string()), error = %e, "sync_stamp_not_merged");
}

/// Applies `ops` to `state` one commit (same HLC stamp) at a time; within a commit, an op that
/// fails is retried after the others, until a round applies nothing new. Returns the ops that
/// never applied, with why.
pub(crate) fn apply_leniently<'a>(
    state: &mut DocState,
    ops: &'a [Op],
) -> Vec<(&'a Op, StateError)> {
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
        | OpKind::RemoveTag { task, .. }
        | OpKind::Move { task, .. } => Some(*task),
        OpKind::NotesEdit { .. } | OpKind::BlankInsert { .. } | OpKind::BlankRemove { .. } => None,
    }
}
