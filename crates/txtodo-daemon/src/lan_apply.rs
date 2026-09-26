//! The two directions `lan_session.rs`'s message loop needs and nothing else: serving a peer's
//! `Want` from the local op log, and committing a peer's `Ops` batch through the right `FileActor`.
//! Split out of `lan_session.rs` purely for the file budget.

use std::collections::BTreeMap;
use std::sync::PoisonError;

use tokio::runtime::Handle;
use txtodo_model::{DeviceId, FilePath, Op};
use txtodo_store::WorkspaceId;
use txtodo_sync::{DevicePublicKey, DeviceSigningKey, Message, OriginRange, sign};

use crate::handle::{ActorError, ActorHandle};
use crate::lan_session::{read, write};
use crate::server::SharedWorkspace;

/// Every `Message::Ops` batch this peer's `Want` produces, chunked to `MAX_OPS_PER_BATCH` (also
/// safely under `txtodo_store::MAX_OPS_PER_READ`, so one `ops_for` call per chunk never refuses).
/// Each op is signed with `signing_key` (see `lan_session.rs`'s module doc and
/// `txtodo_sync::lan_op_signing` for what this stand-in key actually proves).
pub(crate) fn serve_want(
    ws: &SharedWorkspace,
    ranges: &[OriginRange],
    workspace: WorkspaceId,
    signing_key: &DeviceSigningKey,
) -> Result<Vec<Message>, txtodo_store::StoreError> {
    let store = read(ws).store().clone();
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let mut out = Vec::new();
    for r in ranges {
        out.extend(serve_range(&store, r, workspace, signing_key)?);
    }
    Ok(out)
}

/// Signs every op in `ops` in order, dropping (and logging) any op whose canonical bytes cannot be
/// computed rather than sending an unsigned op — `Session::on_ops` would refuse the whole batch on
/// a length mismatch anyway, so failing loud here is no worse and names the op.
fn sign_ops(ops: &[Op], signing_key: &DeviceSigningKey) -> Option<Vec<txtodo_sync::Signature>> {
    ops.iter()
        .map(|op| {
            sign(op, signing_key)
                .inspect_err(|e| tracing::warn!(op = ?op.id, error = %e, "lan_sign_failed"))
                .ok()
        })
        .collect()
}

fn serve_range(
    store: &txtodo_store::Store,
    r: &OriginRange,
    workspace: WorkspaceId,
    signing_key: &DeviceSigningKey,
) -> Result<Vec<Message>, txtodo_store::StoreError> {
    let width = u64::try_from(txtodo_sync::MAX_OPS_PER_BATCH).unwrap_or(u64::MAX);
    let mut out = Vec::new();
    let mut first = r.first;
    while first <= r.last {
        let last = first.saturating_add(width - 1).min(r.last);
        let stored = store.ops_for(r.device, first, last)?;
        let ops: Vec<Op> = stored.into_iter().map(|s| s.op).collect();
        let Some(signatures) = sign_ops(&ops, signing_key) else {
            first = last + 1;
            continue;
        };
        out.push(Message::Ops {
            workspace: workspace.ulid().to_u128(),
            ops,
            signatures,
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

/// Every distinct origin device named by `ops`, mapped to `key` — the stand-in signing key derived
/// from the group key (see `lan_session.rs`'s module doc), so `Session::on_ops`'s `verify_batch`
/// finds an entry for whichever device(s) actually appear in this batch.
pub(crate) fn device_keys_for(
    ops: &[Op],
    key: DevicePublicKey,
) -> BTreeMap<DeviceId, DevicePublicKey> {
    ops.iter().map(|op| (op.hlc.device, key)).collect()
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

fn commit_one_file(ws: &SharedWorkspace, rt: &Handle, path: FilePath, ops: Vec<Op>) -> bool {
    if crate::walker::is_skipped_path(&path) {
        return log_only(ws, &path, &ops);
    }
    // `txtodo.toml` is whole-text too (`layout_sync.rs`); its commit writes the file, and the
    // watcher then hot-reloads the layout.
    if crate::walker::is_notes_document(crate::workspace_mint::basename(&path))
        || crate::layout_sync::is_layout_document(&path)
    {
        return commit_notes_file(ws, &path, ops);
    }
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

/// A peer's ops on a path the walker never walks (a `.claude/worktrees` copy of a repo's backlog,
/// say; task walker-nested-checkouts): into the log, so heads stay dense and other peers still get
/// them, but no file and no actor. A peer's old log can hold tens of thousands of these.
fn log_only(ws: &SharedWorkspace, path: &FilePath, ops: &[Op]) -> bool {
    let store = read(ws).store().clone();
    let appended = store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .append_with_source(ops, Some("sync"));
    match appended {
        Ok(_) => log_logged_only(path, ops.len()),
        Err(e) => log_log_only_failed(path, &e),
    }
}

fn log_logged_only(path: &FilePath, ops: usize) -> bool {
    tracing::debug!(file = %path, ops, "lan_sync_ops_logged_only");
    true
}

fn log_log_only_failed(path: &FilePath, e: &txtodo_store::StoreError) -> bool {
    tracing::warn!(file = %path, error = %e, "lan_sync_log_only_failed");
    false
}

/// A peer's `NotesEdit` ops for one `notes.md` (task notes-sync): the directory is made if this
/// device has never seen it (a nested ref arriving fresh), the notes actor is opened (seeding
/// any bytes already on disk first) and the batch lands through `NotesActor::import_ops`. Used
/// to be dropped silently: `Workspace::register` never builds a `FileActor` for a notes path.
fn commit_notes_file(ws: &SharedWorkspace, path: &FilePath, ops: Vec<Op>) -> bool {
    if !ensure_parent_dir(ws, path) {
        return false;
    }
    let cell = match read(ws).notes_actor(path) {
        Ok(cell) => cell,
        Err(e) => {
            log_refused(path, &e);
            return false;
        }
    };
    let mut actor = cell.lock().unwrap_or_else(PoisonError::into_inner);
    match actor.import_ops(ops) {
        Ok(()) => true,
        Err(e) => {
            log_refused(path, &e);
            false
        }
    }
}

/// Commits `ops` in order, one run of consecutive same-file ops at a time (one commit per file
/// per run — `commit_change_with`'s own invariant), and stops at the first run that fails.
/// Returns how many ops landed, always a prefix of `ops` (task `sync-ack-before-held`,
/// 2026-09-25). The store's head for a device is its op count (`txtodo_store::heads`), so a
/// half-committed batch must never leave a hole: grouping by file and carrying on past a failed
/// file used to commit later ops over a missing earlier one. The caller acks only the prefix
/// ([`landed_ranges`]), and the peer sends the rest again. An op the log already holds counts as
/// landed ([`not_yet_stored`]).
pub(crate) fn commit_incoming_ops(ws: &SharedWorkspace, rt: &Handle, ops: Vec<Op>) -> usize {
    let total = ops.len();
    let mut landed = 0;
    for (path, run) in same_file_runs(ops) {
        let len = run.len();
        if !commit_new_ops(ws, rt, path, run) {
            log_partly_committed(landed, total);
            break;
        }
        landed += len;
    }
    debug_assert!(landed <= total);
    landed
}

/// One same-file run, minus the ops the log already holds. A run of nothing but held ops
/// commits nothing and still counts as landed, so the ack covers it.
fn commit_new_ops(ws: &SharedWorkspace, rt: &Handle, path: FilePath, run: Vec<Op>) -> bool {
    let Some(fresh) = not_yet_stored(ws, &path, run) else {
        return false;
    };
    fresh.is_empty() || commit_one_file(ws, rt, path, fresh)
}

/// `run` without the ops whose id the log already holds (task sync-drift line 2). A device's
/// "op N" is its rank in its own HLC order (`txtodo_store::heads`), and a later own op can sort
/// before ops it already sent, so a batch can start with ops we hold. Inserting one again failed
/// the `UNIQUE` op id and refused the run; the sender resent it every `RESEND_AFTER`, and all
/// later ops from that device waited behind it. A held id whose op differs is skipped too: the
/// log is append-only, so the first copy stays, with a warn. `None` only on a store error.
fn not_yet_stored(ws: &SharedWorkspace, path: &FilePath, run: Vec<Op>) -> Option<Vec<Op>> {
    let store = read(ws).store().clone();
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let total = run.len();
    let mut fresh = Vec::with_capacity(total);
    for op in run {
        match store.op_by_id(op.id) {
            Ok(None) => fresh.push(op),
            Ok(Some(stored)) => warn_if_other(&stored, &op),
            Err(e) => return log_held_check_failed(path, &e),
        }
    }
    debug_assert!(fresh.len() <= total);
    log_already_held(path, total - fresh.len(), total);
    Some(fresh)
}

/// A held id is expected to carry the very op we hold; one that does not is worth a look.
fn warn_if_other(stored: &txtodo_store::Stored, op: &Op) {
    debug_assert_eq!(stored.op.id, op.id);
    if stored.op != *op {
        log_id_conflict(stored, op);
    }
}

/// Split out of [`warn_if_other`]: a macro call inside a branch costs cognitive complexity.
fn log_id_conflict(stored: &txtodo_store::Stored, op: &Op) {
    tracing::warn!(
        file = %op.file,
        op = %op.id.ulid(),
        seq = stored.seq.0,
        kind = txtodo_store::kind_tag(&op.kind),
        stored_kind = txtodo_store::kind_tag(&stored.op.kind),
        "lan_sync_op_id_conflict"
    );
}

/// Info, not debug: a held op in a batch means the sender's ranks moved (sync-drift line 6),
/// and the op that moved them may never reach us.
fn log_already_held(path: &FilePath, held: usize, total: usize) {
    if held > 0 {
        tracing::info!(file = %path, held, total, "lan_sync_ops_already_held");
    }
}

fn log_held_check_failed(path: &FilePath, e: &txtodo_store::StoreError) -> Option<Vec<Op>> {
    tracing::warn!(file = %path, error = %e, "lan_sync_held_check_failed");
    None
}

/// `ops` cut into maximal runs of consecutive ops on one file, in order.
fn same_file_runs(ops: Vec<Op>) -> Vec<(FilePath, Vec<Op>)> {
    let mut runs: Vec<(FilePath, Vec<Op>)> = Vec::new();
    for op in ops {
        match runs.last_mut() {
            Some((path, run)) if *path == op.file => run.push(op),
            _ => runs.push((op.file.clone(), vec![op])),
        }
    }
    debug_assert!(
        runs.windows(2).all(|w| w[0].0 != w[1].0),
        "runs are maximal"
    );
    debug_assert!(runs.iter().all(|(_, run)| !run.is_empty()));
    runs
}

/// The runs covering the first `landed` ops of a batch whose ops follow `ranges` in order (one op
/// per origin seq, `serve_range`'s own shape). A batch that carried fewer ops than its ranges name
/// is acked only for the ops it carried, so heads never run past the store's count.
pub(crate) fn landed_ranges(ranges: &[OriginRange], landed: usize) -> Vec<OriginRange> {
    let mut left = u64::try_from(landed).unwrap_or(u64::MAX);
    let mut out = Vec::new();
    for r in ranges {
        if left == 0 {
            break;
        }
        let take = (r.last - r.first + 1).min(left);
        out.push(OriginRange {
            device: r.device,
            first: r.first,
            last: r.first + take - 1,
        });
        left -= take;
    }
    debug_assert!(out.len() <= ranges.len());
    debug_assert!(
        out.iter()
            .zip(ranges)
            .all(|(o, r)| o.device == r.device && o.first == r.first && o.last <= r.last),
        "each landed run is a prefix of its range"
    );
    out
}

fn log_partly_committed(landed: usize, total: usize) {
    tracing::warn!(landed, total, "lan_sync_batch_partly_committed");
}

fn log_refused(path: &FilePath, e: &ActorError) {
    tracing::warn!(file = %path, error = %e, "lan_sync_ops_refused");
}
