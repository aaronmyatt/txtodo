//! Keeps a `notes.md`'s op log able to rebuild its text (task notes-no-base). Two halves:
//!
//! - [`WaitingEdits`]: a peer's notes op that does not fit waits and is retried after each later
//!   op that lands; only one pushed past [`MAX_WAITING_NOTES_OPS`] is skipped, with a
//!   `sync_op_skipped` warn. It may build on another device's edit that has not arrived: since own
//!   carries across (ADR 0029's amendment) a device takes ops from several peers at once (lab chaos
//!   20261002-235331: b1 skipped a2's append made on top of a1's, which came a moment later). It
//!   used to be skipped at once; before that, refusing it held up every later op of its device.
//! - [`repair_edits`]: the edits that take "the log replayed from empty" to the current text. A
//!   notes.md opened before notes-sync (v0.0.8) had its bytes adopted with no op, so its first
//!   logged op can sit at char 8874 of a text no op ever wrote. `NotesActor::open` commits these
//!   edits as one op, so a fresh peer that replays the same ops the same way lands on the file.

use std::collections::VecDeque;

use crate::handle::ActorError;
use crate::history::MAX_REPLAY_PAGES;
use crate::notes_state::{NotesState, NotesStateError};
use txtodo_model::{FilePath, Op, TextEdit};
use txtodo_store::{MAX_OPS_PER_READ, Seq, Store};

/// Most notes ops one document keeps waiting; past it the oldest is skipped.
pub(crate) const MAX_WAITING_NOTES_OPS: usize = 1_000;

/// Notes ops that did not fit yet, oldest first. Import and replay both apply through this, so a
/// peer that imports in batches and a device that replays its log reach the same text.
#[derive(Clone, Debug, Default)]
pub(crate) struct WaitingEdits {
    ops: VecDeque<Op>,
}

impl WaitingEdits {
    /// Whether any op waits: the text is about to change, so a digest says nothing yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Applies `ops` to `state` in order; one that does not fit leaves `state` as it was
    /// (`NotesState::apply`'s own contract) and waits. After each op that lands, the waiting ones
    /// are tried again. Returns the ops pushed past the bound, with why: those are skipped.
    pub(crate) fn apply<'a>(
        &mut self,
        state: &mut NotesState,
        ops: impl IntoIterator<Item = &'a Op>,
    ) -> Vec<(Op, NotesStateError)> {
        let mut skipped = Vec::new();
        for op in ops {
            match state.apply(op) {
                Ok(()) => self.retry(state),
                Err(e) => {
                    log_waiting(op, &e);
                    self.ops.push_back(op.clone());
                    if self.ops.len() > MAX_WAITING_NOTES_OPS
                        && let Some(oldest) = self.ops.pop_front()
                    {
                        skipped.push((oldest, e));
                    }
                }
            }
        }
        debug_assert!(self.ops.len() <= MAX_WAITING_NOTES_OPS);
        skipped
    }

    /// Tries every waiting op until a round lands none. Bounded: each round but the last lands
    /// one, and an op that lands leaves the queue.
    fn retry(&mut self, state: &mut NotesState) {
        loop {
            let before = self.ops.len();
            self.ops.retain(|op| state.apply(op).is_err());
            if self.ops.len() == before {
                return;
            }
        }
    }
}

/// tracing `debug!`: <https://docs.rs/tracing/latest/tracing/macro.debug.html>
fn log_waiting(op: &Op, e: &NotesStateError) {
    tracing::debug!(file = %op.file, op = %op.id.ulid(), error = %e, "notes_op_waiting");
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
    /// The ops still waiting at the end, for the actor to keep waiting on.
    pub(crate) waiting: WaitingEdits,
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
    let mut waiting = WaitingEdits::default();
    let mut since = Seq(0);
    for _page in 0..MAX_REPLAY_PAGES {
        let ops = store.for_file(path, since)?;
        let Some(last) = ops.last() else {
            return Ok(Replayed {
                state,
                complete: true,
                waiting,
            });
        };
        let upto_target = ops.iter().take_while(|s| s.seq <= target).map(|s| &s.op);
        waiting.apply(&mut state, upto_target);
        since = last.seq;
        if last.seq >= target || ops.len() < MAX_OPS_PER_READ {
            return Ok(Replayed {
                state,
                complete: true,
                waiting,
            });
        }
    }
    Ok(Replayed {
        state,
        complete: false,
        waiting,
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
