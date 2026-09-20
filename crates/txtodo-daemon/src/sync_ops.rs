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
use txtodo_model::Op;

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
    /// document, same invariant `commit_change_with` asserts) in the order they arrived, which the
    /// wire protocol's own doc guarantees is "a total order the receiver may apply as-is". A
    /// refusal partway through means the batch never commits: the caller then acks nothing, so the
    /// peer resends the whole run (`Session::committed`'s own documented behaviour).
    pub(crate) fn on_sync_ops(&mut self, ops: Vec<Op>) -> Result<(), ActorError> {
        debug_assert!(
            ops.iter().all(|o| o.file == self.cfg.path),
            "the caller routes by op.file before calling here"
        );
        if ops.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        for op in &ops {
            next.apply(op)?;
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
