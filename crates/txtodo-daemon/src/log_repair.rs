//! Keeps a task document's op log able to rebuild its file (task todo-log-repair), the
//! `todo.txt` counterpart of `notes_repair.rs`.
//!
//! A peer only ever gets the ops. When this device's reconcile could not express an edit exactly
//! (`exact: false`), it adopted the file and pinned it with a snapshot; the snapshot never syncs,
//! so a peer kept the reconciler's lossier text. Seen on 2026-09-28: old `txtodo do` runs replayed
//! as `x D desc` where the file had `x D D desc`, on 79 lists of this repo's own backlog.
//!
//! At open, [`FileActor::repair_log`] replays the log from empty, ignoring snapshots (what a peer
//! has) and skipping what does not fit the way `on_sync_ops` does. When that renders other bytes
//! than the projection, it commits `reconcile_replay::replayable_ops(replayed, current)`: ops by
//! task id that take the replayed state to this one, each checked on a scratch copy. The file and
//! the projection stay as they are, and a snapshot is forced at the repair's seq so
//! `history::replay` starts from the file instead of applying the repair on top of an older pin.

use crate::actor::FileActor;
use crate::commit::{Commit, CommitTail};
use crate::handle::ActorError;
use crate::history::MAX_REPLAY_PAGES;
use crate::reconcile_replay::replayable_ops;
use crate::state::DocState;
use crate::sync_ops::apply_leniently;
use txtodo_core::File;
use txtodo_model::{FilePath, Hlc, IdentityMode, Principal};
use txtodo_store::{MAX_OPS_PER_READ, Seq, Store};

impl FileActor {
    /// Commits the ops a peer needs to rebuild this file from the log, when the log alone does
    /// not. A no-op for a log that already rebuilds it, a replay cut off at `MAX_REPLAY_PAGES`, or
    /// a difference `replayable_ops` cannot express (logged).
    pub(crate) fn repair_log(&mut self) -> Result<(), ActorError> {
        let replayed = {
            let store = self.lock_store();
            replay_from_empty(&store, &self.cfg.path, self.cfg.identity_mode)?
        };
        let Some(replayed) = replayed else {
            return Ok(());
        };
        if replayed.to_bytes() == self.projection {
            self.adopt_replayed_stamps(&replayed);
            return Ok(());
        }
        let Some((kinds, work)) = replayable_ops(&replayed, &self.state) else {
            log_unrepairable(&self.cfg.path);
            self.adopt_replayed_stamps(&replayed);
            return Ok(());
        };
        debug_assert_eq!(
            work.to_bytes(),
            self.projection,
            "checked on the scratch copy"
        );
        log_repaired(&self.cfg.path, kinds.len());
        let principal = Principal::External {
            device: self.cfg.device,
        };
        // The repair's ops must be newer than every line the replay placed, or a peer would slot
        // them past those lines instead of where the scratch copy put them.
        self.catch_up_clock(replayed.newest_stamp());
        let ops = self.stamp(kinds, &principal)?;
        let mut rebuilt = replayed;
        apply_leniently(&mut rebuilt, &ops);
        self.commit(Commit {
            ops,
            next: self.state.clone(),
            bytes: self.projection.clone(),
            write: false,
            snapshot: true,
            tail: CommitTail {
                source: Some("repair".to_owned()),
                ..CommitTail::default()
            },
        })?;
        self.adopt_replayed_stamps(&rebuilt);
        Ok(())
    }

    /// The state was read from disk, so no line knows which op placed it; `replayed` (the log
    /// rebuilt from empty, what a peer holds) does (task insert-order). The clock then moves past
    /// the newest of those stamps, so this device's next op still sorts after every line.
    fn adopt_replayed_stamps(&mut self, replayed: &DocState) {
        self.state.adopt_stamps(replayed);
        self.catch_up_clock(self.state.newest_stamp());
    }

    /// Moves the clock up to `newest` (never back), keeping this device's id. Same adoption as
    /// `recover` makes of the newest stored op.
    fn catch_up_clock(&mut self, newest: Option<Hlc>) {
        if let Some(newest) = newest.filter(|n| *n > self.hlc) {
            self.hlc = Hlc {
                device: self.cfg.device,
                ..newest
            };
        }
        debug_assert_eq!(self.hlc.device, self.cfg.device);
    }
}

/// `path`'s state after every logged op, applied from an empty document in log order and
/// leniently, with no snapshot. `None` when the log runs past `MAX_REPLAY_PAGES` pages: a
/// cut-off replay is not the log's text, and a repair must never act on one.
pub(crate) fn replay_from_empty(
    store: &Store,
    path: &FilePath,
    mode: IdentityMode,
) -> Result<Option<DocState>, ActorError> {
    let mut state = DocState::from_file(path.clone(), &File::default(), &[], mode)?;
    let mut since = Seq(0);
    for _page in 0..MAX_REPLAY_PAGES {
        let stored = store.for_file(path, since)?;
        let Some(last) = stored.last() else {
            return Ok(Some(state));
        };
        since = last.seq;
        let full = stored.len() >= MAX_OPS_PER_READ;
        let ops: Vec<_> = stored.into_iter().map(|s| s.op).collect();
        apply_leniently(&mut state, &ops);
        if !full {
            return Ok(Some(state));
        }
    }
    Ok(None)
}

/// tracing `warn!`: <https://docs.rs/tracing/latest/tracing/macro.warn.html>
fn log_repaired(path: &FilePath, ops: usize) {
    tracing::warn!(file = %path, ops, "todo_log_repaired");
}

fn log_unrepairable(path: &FilePath) {
    tracing::warn!(file = %path, "todo_log_unrepairable");
}
