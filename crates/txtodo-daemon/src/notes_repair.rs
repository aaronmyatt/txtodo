//! A `notes.md` op that does not fit is skipped, not refused (task notes-no-base).
//!
//! [`apply_leniently`]: a peer's notes op that does not fit is skipped with a `sync_op_skipped`
//! warn instead of refusing the batch. A refused op used to be resent every 10 s forever, and
//! every later op from that device, in every file of the workspace, waited behind it. Same rule
//! `FileActor::on_sync_ops` has had for todo.txt since sync-poison-op. History replay skips the
//! same way ([`replay_leniently`]), so a device replaying its log reaches the text it imported.

use crate::handle::ActorError;
use crate::history::MAX_REPLAY_PAGES;
use crate::notes_state::{NotesState, NotesStateError};
use txtodo_model::{FilePath, Op};
use txtodo_store::{MAX_OPS_PER_READ, Seq, Store};

/// Applies `ops` to `state` in order; one that does not fit leaves `state` as it was
/// (`NotesState::apply`'s own contract) and is returned with why. Import and replay both use this,
/// so a peer that imports a batch and a device that replays its log reach the same text.
pub(crate) fn apply_leniently<'a>(
    state: &mut NotesState,
    ops: impl IntoIterator<Item = &'a Op>,
) -> Vec<(&'a Op, NotesStateError)> {
    let mut skipped = Vec::new();
    for op in ops {
        if let Err(e) = state.apply(op) {
            skipped.push((op, e));
        }
    }
    skipped
}

/// The warn `FileActor`'s import logs for a skipped op, with the same event name and fields
/// (tracing `Event` docs: <https://docs.rs/tracing/latest/tracing/macro.warn.html>).
pub(crate) fn log_skipped(op: &Op, e: &NotesStateError) {
    tracing::warn!(
        file = %op.file,
        op = %op.id.ulid(),
        kind = txtodo_store::kind_tag(&op.kind),
        error = %e,
        "sync_op_skipped"
    );
}

/// `path`'s text after every logged op with `seq <= upto` (all when `None`), skipping what does
/// not fit.
pub(crate) fn replay_leniently(
    store: &Store,
    path: &FilePath,
    upto: Option<Seq>,
) -> Result<NotesState, ActorError> {
    let target = match upto {
        Some(s) => s,
        None => store.last_seq()?.unwrap_or(Seq(0)),
    };
    let mut state = NotesState::empty(path.clone());
    let mut since = Seq(0);
    for _page in 0..MAX_REPLAY_PAGES {
        let ops = store.for_file(path, since)?;
        let Some(last) = ops.last() else {
            return Ok(state);
        };
        let upto_target = ops.iter().take_while(|s| s.seq <= target).map(|s| &s.op);
        apply_leniently(&mut state, upto_target);
        since = last.seq;
        if last.seq >= target || ops.len() < MAX_OPS_PER_READ {
            return Ok(state);
        }
    }
    Ok(state)
}
