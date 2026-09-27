//! Keeps a `notes.md`'s op log able to rebuild its text (task notes-no-base). Two halves:
//!
//! - [`apply_leniently`]: a peer's notes op that does not fit is skipped with a `sync_op_skipped`
//!   warn instead of refusing the batch. A refused op used to be resent every 10 s forever, and
//!   every later op from that device, in every file of the workspace, waited behind it. Same rule
//!   `FileActor::on_sync_ops` has had for todo.txt since sync-poison-op.
//! - [`repair_edits`]: the edits that take "the log replayed from empty" to the current text. A
//!   notes.md opened before notes-sync (v0.0.8) had its bytes adopted with no op, so its first
//!   logged op can sit at char 8874 of a text no op ever wrote. `NotesActor::open` commits these
//!   edits as one op, so a fresh peer that replays the same ops the same way lands on the file.

use crate::handle::ActorError;
use crate::history::MAX_REPLAY_PAGES;
use crate::notes_state::{NotesState, NotesStateError};
use txtodo_model::{FilePath, Op, TextEdit};
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

/// A replay's text, and whether it read the whole log up to its target.
pub(crate) struct Replayed {
    /// The text.
    pub(crate) state: NotesState,
    /// `false` when the log ran past `MAX_REPLAY_PAGES` pages: a cut-off replay is not the log's
    /// text, and [`repair_edits`] must never act on one.
    pub(crate) complete: bool,
}

/// `path`'s text after every logged op with `seq <= upto` (all when `None`), skipping what does
/// not fit.
pub(crate) fn replay_leniently(
    store: &Store,
    path: &FilePath,
    upto: Option<Seq>,
) -> Result<Replayed, ActorError> {
    let target = match upto {
        Some(s) => s,
        None => store.last_seq()?.unwrap_or(Seq(0)),
    };
    let mut state = NotesState::empty(path.clone());
    let mut since = Seq(0);
    for _page in 0..MAX_REPLAY_PAGES {
        let ops = store.for_file(path, since)?;
        let Some(last) = ops.last() else {
            return Ok(Replayed {
                state,
                complete: true,
            });
        };
        let upto_target = ops.iter().take_while(|s| s.seq <= target).map(|s| &s.op);
        apply_leniently(&mut state, upto_target);
        since = last.seq;
        if last.seq >= target || ops.len() < MAX_OPS_PER_READ {
            return Ok(Replayed {
                state,
                complete: true,
            });
        }
    }
    Ok(Replayed {
        state,
        complete: false,
    })
}

/// The edits that take `replayed` (the log's text) to `current` (the file), or `None` when the
/// log already rebuilds the file.
pub(crate) fn repair_edits(replayed: &NotesState, current: &NotesState) -> Option<Vec<TextEdit>> {
    if replayed.text() == current.text() {
        return None;
    }
    let edits: Vec<TextEdit> = txtodo_core::diff_text(replayed.text(), current.text())
        .into_iter()
        .map(TextEdit::from)
        .collect();
    debug_assert!(
        !edits.is_empty(),
        "different texts diff to at least one edit"
    );
    Some(edits)
}
