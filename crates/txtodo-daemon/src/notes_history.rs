//! History as a view over a `notes.md` (plan M5), the notes analogue of `history.rs`: replay to a
//! log position, render at a wall time, invert edits for undo. `history::seq_at_wall` is reused
//! as-is — it only walks store rows by HLC wall time, no document shape involved.

use crate::handle::ActorError;
use crate::history::{MAX_REPLAY_PAGES, seq_at_wall};
use crate::notes_state::{NotesState, NotesStateError};
use crate::textedit::apply_notes_edits;
use txtodo_model::{FilePath, OpKind, TextEdit};
use txtodo_store::{MAX_OPS_PER_READ, Seq, Store, Stored};

impl From<NotesStateError> for ActorError {
    fn from(e: NotesStateError) -> ActorError {
        ActorError::Notes(e)
    }
}

/// The document state after every `NotesEdit` with `seq <= upto` (all when `upto` is `None`). No
/// snapshot optimisation yet (plan M5 MVP): a notes.md's op volume is far below a task document's.
pub fn replay(store: &Store, path: &FilePath, upto: Option<Seq>) -> Result<NotesState, ActorError> {
    let target = match upto {
        Some(s) => s,
        None => store.last_seq()?.unwrap_or(Seq(0)),
    };
    let mut state = NotesState::empty(path.clone());
    let mut since = Seq(0);
    for _page in 0..MAX_REPLAY_PAGES {
        let ops = store.for_file(path, since)?;
        let Some(last) = ops.last() else { break };
        for stored in ops.iter().take_while(|s| s.seq <= target) {
            state.apply(&stored.op)?;
        }
        since = last.seq;
        if last.seq >= target || ops.len() < MAX_OPS_PER_READ {
            break;
        }
    }
    Ok(state)
}

/// The document bytes as they were at `at_wall_ms` (inclusive).
pub fn checkout(store: &Store, path: &FilePath, at_wall_ms: u64) -> Result<Vec<u8>, ActorError> {
    let Some(seq) = seq_at_wall(store, path, at_wall_ms)? else {
        return Ok(Vec::new());
    };
    Ok(replay(store, path, Some(seq))?.to_bytes())
}

/// The op that undoes `stored`, given the state just before it.
pub fn inverse(before: &NotesState, stored: &Stored) -> Option<OpKind> {
    let OpKind::NotesEdit { file, edits } = &stored.op.kind else {
        return None;
    };
    let old = before.text().to_owned();
    let new = apply_notes_edits(&old, edits).ok()?;
    let back: Vec<TextEdit> = txtodo_core::diff_text(&new, &old)
        .into_iter()
        .map(TextEdit::from)
        .collect();
    debug_assert!(apply_notes_edits(&new, &back).ok().as_deref() == Some(old.as_str()));
    Some(OpKind::NotesEdit {
        file: file.clone(),
        edits: back,
    })
}

/// Inverse edits for the newest `steps` ops of `path`, newest first.
pub fn undo_ops(store: &Store, path: &FilePath, steps: u16) -> Result<Vec<OpKind>, ActorError> {
    let steps = usize::from(steps.max(1));
    let newest = store.newest(path, steps)?;
    let mut out = Vec::with_capacity(newest.len());
    for stored in &newest {
        let before = replay(store, path, Some(Seq(stored.seq.0 - 1)))?;
        if let Some(inv) = inverse(&before, stored) {
            out.push(inv);
        }
    }
    debug_assert!(out.len() <= steps);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::{DeviceId, Hlc, Op, OpId, Principal, Ulid};

    fn path() -> FilePath {
        FilePath::new("q4/abc/notes.md").unwrap_or_else(|e| panic!("{e}"))
    }

    fn op(n: u128, wall_ms: u64, edits: Vec<TextEdit>) -> Op {
        let device = DeviceId::new(Ulid::from_u128(7));
        Op {
            id: OpId::new(Ulid::from_u128(n)),
            hlc: Hlc {
                wall_ms,
                counter: 0,
                device,
            },
            principal: Principal::User { device },
            file: path(),
            kind: OpKind::NotesEdit {
                file: path(),
                edits,
            },
        }
    }

    fn seeded() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let mut store = Store::open(&dir.path().join("oplog.db")).unwrap_or_else(|e| panic!("{e}"));
        let ops = [
            op(
                1,
                1000,
                vec![TextEdit::Insert {
                    at: 0,
                    text: "hello".into(),
                }],
            ),
            op(
                2,
                2000,
                vec![TextEdit::Insert {
                    at: 5,
                    text: " world".into(),
                }],
            ),
        ];
        store.append(&ops).unwrap_or_else(|e| panic!("{e}"));
        (dir, store)
    }

    #[test]
    fn replay_and_checkout_render_intermediate_states() {
        let (_dir, store) = seeded();
        assert_eq!(
            replay(&store, &path(), None)
                .unwrap_or_else(|e| panic!("{e}"))
                .text(),
            "hello world"
        );
        assert_eq!(
            replay(&store, &path(), Some(Seq(1)))
                .unwrap_or_else(|e| panic!("{e}"))
                .text(),
            "hello"
        );
        assert_eq!(
            checkout(&store, &path(), 500).unwrap_or_else(|e| panic!("{e}")),
            b""
        );
        assert_eq!(
            checkout(&store, &path(), 1500).unwrap_or_else(|e| panic!("{e}")),
            b"hello"
        );
    }

    #[test]
    fn undo_ops_are_newest_first_and_invert_the_text() {
        let (_dir, store) = seeded();
        let inverses = undo_ops(&store, &path(), 2).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(inverses.len(), 2);
        let mut state = replay(&store, &path(), None).unwrap_or_else(|e| panic!("{e}"));
        for inv in &inverses {
            let OpKind::NotesEdit { edits, .. } = inv else {
                panic!("expected a NotesEdit");
            };
            state.apply_edits(edits).unwrap_or_else(|e| panic!("{e}"));
        }
        assert_eq!(state.text(), "");
    }
}
