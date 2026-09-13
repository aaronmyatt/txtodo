//! The two directions `lan_session.rs`'s message loop needs and nothing else: serving a peer's
//! `Want` from the local op log, and committing a peer's `Ops` batch through the right `FileActor`.
//! Split out of `lan_session.rs` purely for the file budget.

use std::collections::BTreeMap;
use std::sync::PoisonError;

use tokio::runtime::Handle;
use txtodo_model::{FilePath, Op};
use txtodo_sync::{Message, OriginRange};

use crate::handle::{ActorError, ActorHandle};
use crate::lan_session::{read, write};
use crate::server::SharedWorkspace;

/// Every `Message::Ops` batch this peer's `Want` produces, chunked to `MAX_OPS_PER_BATCH` (also
/// safely under `txtodo_store::MAX_OPS_PER_READ`, so one `ops_for` call per chunk never refuses).
pub(crate) fn serve_want(
    ws: &SharedWorkspace,
    ranges: &[OriginRange],
) -> Result<Vec<Message>, txtodo_store::StoreError> {
    let store = read(ws).store().clone();
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let mut out = Vec::new();
    for r in ranges {
        out.extend(serve_range(&store, r)?);
    }
    Ok(out)
}

fn serve_range(
    store: &txtodo_store::Store,
    r: &OriginRange,
) -> Result<Vec<Message>, txtodo_store::StoreError> {
    let width = u64::try_from(txtodo_sync::MAX_OPS_PER_BATCH).unwrap_or(u64::MAX);
    let mut out = Vec::new();
    let mut first = r.first;
    while first <= r.last {
        let last = first.saturating_add(width - 1).min(r.last);
        let stored = store.ops_for(r.device, first, last)?;
        let ops: Vec<Op> = stored.into_iter().map(|s| s.op).collect();
        out.push(Message::Ops {
            ops,
            ranges: vec![OriginRange {
                device: r.device,
                first,
                last,
            }],
        });
        first = last + 1;
    }
    Ok(out)
}

fn ensure_parent_dir(ws: &SharedWorkspace, path: &FilePath) -> bool {
    let disk = read(ws).root().join(path.as_str());
    let Some(parent) = disk.parent() else {
        return true;
    };
    match std::fs::create_dir_all(parent) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(file = %path, error = %e, "lan_sync_mkdir_failed");
            false
        }
    }
}

fn register_and_fetch(ws: &SharedWorkspace, path: &FilePath) -> Option<ActorHandle> {
    let mut guard = write(ws);
    if let Err(e) = guard.register(path.clone()) {
        tracing::warn!(file = %path, error = %e, "lan_sync_register_failed");
        return None;
    }
    guard.actor(path).cloned()
}

/// The actor for `path`, registering (and, for a nested-ref file arriving for the first time on a
/// fresh device, creating the containing directory for) a not-yet-known document. `None` only on a
/// real failure, logged with the file named.
fn get_or_create_actor(ws: &SharedWorkspace, path: &FilePath) -> Option<ActorHandle> {
    if let Some(h) = read(ws).actor(path) {
        return Some(h.clone());
    }
    if !ensure_parent_dir(ws, path) {
        return None;
    }
    register_and_fetch(ws, path)
}

fn group_ops_by_file(ops: Vec<Op>) -> BTreeMap<FilePath, Vec<Op>> {
    let mut by_file: BTreeMap<FilePath, Vec<Op>> = BTreeMap::new();
    for op in ops {
        by_file.entry(op.file.clone()).or_default().push(op);
    }
    by_file
}

fn commit_one_file(ws: &SharedWorkspace, rt: &Handle, path: FilePath, ops: Vec<Op>) -> bool {
    let Some(handle) = get_or_create_actor(ws, &path) else {
        return false;
    };
    match rt.block_on(handle.sync_import_ops(ops)) {
        Ok(()) => true,
        Err(e) => {
            log_refused(&path, &e);
            false
        }
    }
}

/// Routes `ops` to their document's actor (one commit per file — `commit_change_with`'s own
/// invariant) and commits each group. `true` only if every group committed; the caller must ack
/// nothing at all otherwise, so the peer resends the whole batch (`Session::committed`'s own
/// documented crash-then-nothing-committed behaviour, extended here to a partial-file failure).
pub(crate) fn commit_incoming_ops(ws: &SharedWorkspace, rt: &Handle, ops: Vec<Op>) -> bool {
    let mut all_ok = true;
    for (path, file_ops) in group_ops_by_file(ops) {
        if !commit_one_file(ws, rt, path, file_ops) {
            all_ok = false;
        }
    }
    all_ok
}

fn log_refused(path: &FilePath, e: &ActorError) {
    tracing::warn!(file = %path, error = %e, "lan_sync_ops_refused");
}
