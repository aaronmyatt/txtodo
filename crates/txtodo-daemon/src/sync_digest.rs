//! File digests between paired devices (ADR 0035, task sync-divergence-check). When a live session
//! goes quiet, each side sends, per workspace, every file's op-set hash (`op_set_hash.rs`) and
//! byte hash; the receiver books a file whose op sets match and bytes do not as split with that
//! peer (`split_files.rs`), and clears it when a later digest agrees. Report, never heal.
//!
//! - Sent once per workspace when the session has heard nothing for [`DIGEST_QUIET`], nothing is in
//!   flight or owed to that workspace, and it committed something since its last digest (or never
//!   sent one on this connection).
//! - A file with peer ops waiting for another device's insert (`sync_park.rs`) is left out, by the
//!   sender, and skipped by the receiver: it is about to change.
//! - Different op sets say nothing: one side is behind, which the sync itself handles.

use std::collections::BTreeMap;
use std::sync::PoisonError;
use std::time::Duration;

use tokio::runtime::Handle;
use txtodo_model::{DeviceId, FilePath};
use txtodo_store::WorkspaceId;
use txtodo_sync::{FileDigest, GroupId, GroupKey, Link, MAX_DIGEST_FILES, Message};

use crate::device_relay::WorkspaceRoute;
use crate::expected::{Hash, hex8};
use crate::lan_session::read;
use crate::lan_session_live::Live;
use crate::lan_session_shared::{SessionCtx, send_message};
use crate::op_set_hash::OpSetHash;
use crate::server::SharedWorkspace;

/// Silence after which a session counts as quiet: above a round trip, below `HEARTBEAT` (5 s), so
/// the gap between heartbeats is quiet enough.
#[cfg(not(test))]
pub(crate) const DIGEST_QUIET: Duration = Duration::from_secs(1);
#[cfg(test)]
pub(crate) const DIGEST_QUIET: Duration = Duration::from_millis(100);

/// One document's two hashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocDigest {
    /// Every op the document's log holds, order-free.
    pub ops: OpSetHash,
    /// blake3 of the bytes this device renders.
    pub bytes: Hash,
}

/// `path`'s hashes here: its file actor's, else an open notes actor's. `None` for a file with peer
/// ops waiting, or one this device does not have open.
pub(crate) fn local_digest(
    ws: &SharedWorkspace,
    rt: &Handle,
    path: &FilePath,
) -> Option<DocDigest> {
    let actor = read(ws).actor(path).cloned();
    if let Some(handle) = actor {
        return rt.block_on(handle.digest()).ok().flatten();
    }
    let cell = read(ws).notes_cell(path)?;
    let notes = cell.lock().unwrap_or_else(PoisonError::into_inner);
    Some(DocDigest {
        ops: notes.op_set(),
        bytes: notes.contents().1,
    })
}

/// Every file of `ws` this device can vouch for, lists and notes alike.
pub(crate) fn workspace_files(ws: &SharedWorkspace, rt: &Handle) -> Vec<FileDigest> {
    let paths: Vec<FilePath> = {
        let w = read(ws);
        w.paths().cloned().chain(w.notes_paths()).collect()
    };
    paths
        .into_iter()
        .filter_map(|path| {
            let d = local_digest(ws, rt, &path)?;
            Some(FileDigest {
                path: path.as_str().to_owned(),
                ops: *d.ops.as_bytes(),
                bytes: d.bytes,
            })
        })
        .collect()
}

/// `files` as `Digest` messages for `workspace`, at most [`MAX_DIGEST_FILES`] each.
pub(crate) fn digest_messages(workspace: WorkspaceId, files: &[FileDigest]) -> Vec<Message> {
    files
        .chunks(MAX_DIGEST_FILES)
        .map(|chunk| Message::Digest {
            workspace: workspace.ulid().to_u128(),
            files: chunk.to_vec(),
        })
        .collect()
}

/// A peer's `Digest` for `ctx.workspace`: books each file whose op set matches ours and bytes do
/// not, clears each that matches in both. `peer` is `None` before the link `Hello`: nothing to book.
pub(crate) fn on_digest(ctx: &SessionCtx<'_>, peer: Option<DeviceId>, files: &[FileDigest]) {
    let Some(peer) = peer else {
        return;
    };
    let compared: Vec<bool> = files
        .iter()
        .filter_map(|theirs| compare_one(ctx, peer, theirs))
        .collect();
    let split = compared.iter().filter(|s| **s).count();
    debug_assert!(compared.len() <= files.len());
    tracing::debug!(
        %peer,
        workspace = %ctx.workspace,
        files = files.len(),
        compared = compared.len(),
        split,
        "sync_digest_compared"
    );
}

/// One file of a peer's digest against ours: `None` when there is nothing to compare (unknown
/// path, a file we do not hold or hold with ops waiting, or different op sets), else whether it is
/// split. Books or clears it.
fn compare_one(ctx: &SessionCtx<'_>, peer: DeviceId, theirs: &FileDigest) -> Option<bool> {
    let path = FilePath::new(&theirs.path).ok()?;
    let ours = local_digest(ctx.ws, ctx.rt, &path)?;
    if ours.ops.as_bytes() != &theirs.ops {
        return None;
    }
    let (splits, now) = {
        let w = read(ctx.ws);
        (w.split_files().clone(), w.clock().now_ms())
    };
    let split = ours.bytes != theirs.bytes;
    if split && splits.book(peer, ctx.workspace, path.clone(), now) {
        log_split_found(peer, ctx.workspace, &path, &ours.bytes, &theirs.bytes);
    }
    if !split && splits.clear(peer, ctx.workspace, &path) {
        log_split_cleared(peer, ctx.workspace, &path);
    }
    Some(split)
}

/// What the sender needs from the dispatch loop's context.
pub(crate) struct DigestCtx<'a> {
    pub(crate) rt: &'a Handle,
    pub(crate) group: GroupId,
    pub(crate) key: &'a GroupKey,
    pub(crate) routes: &'a BTreeMap<WorkspaceId, WorkspaceRoute>,
}

/// One connection's digest bookkeeping: per workspace, its commit count at its last digest.
#[derive(Default)]
pub(crate) struct DigestSender {
    sent_at: BTreeMap<WorkspaceId, u64>,
}

impl DigestSender {
    /// Sends a digest for each workspace that is quiet and changed since its last one (module
    /// doc). `false` only on a failed send.
    pub(crate) fn tick(&mut self, link: &mut dyn Link, live: &Live, ctx: &DigestCtx<'_>) -> bool {
        if live.quiet_for() < DIGEST_QUIET {
            return true;
        }
        ctx.routes
            .iter()
            .filter(|(id, _)| live.idle(**id))
            .all(|(id, route)| self.send_if_changed(link, ctx, *id, route))
    }

    /// `id`'s digest, when it committed something since its last one. `false` only on a failed send.
    fn send_if_changed(
        &mut self,
        link: &mut dyn Link,
        ctx: &DigestCtx<'_>,
        id: WorkspaceId,
        route: &WorkspaceRoute,
    ) -> bool {
        let commits = read(&route.ws).stats().commits();
        if self.sent_at.get(&id) == Some(&commits) {
            return true;
        }
        let files = workspace_files(&route.ws, ctx.rt);
        for msg in digest_messages(id, &files) {
            if send_message(link, ctx.group, id, ctx.key, msg).is_err() {
                return false;
            }
        }
        self.sent_at.insert(id, commits);
        tracing::debug!(workspace = %id, files = files.len(), "sync_digest_sent");
        true
    }
}

fn log_split_found(
    peer: DeviceId,
    workspace: WorkspaceId,
    file: &FilePath,
    ours: &Hash,
    theirs: &Hash,
) {
    tracing::warn!(
        %peer,
        %workspace,
        %file,
        ours = %hex8(ours),
        theirs = %hex8(theirs),
        "sync_split_found"
    );
}

fn log_split_cleared(peer: DeviceId, workspace: WorkspaceId, file: &FilePath) {
    tracing::info!(%peer, %workspace, %file, "sync_split_cleared");
}
