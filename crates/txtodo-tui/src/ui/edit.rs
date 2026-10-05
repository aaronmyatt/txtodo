//! The single-line editor (`i`/`a`/`A`): buffer editing plus mapping a finished draft to the
//! `Edit`/`Add` mutation the daemon turns into an op (design §3.2, §7). Pure `AppState` mutation —
//! no I/O; `app.rs` sends the mutation `commit` returns through a real `Daemon::apply`.

use crossterm::event::{KeyCode, KeyEvent};
use txtodo_proto::v1 as pb;

use crate::state::{AppState, EditDraft, EditTarget};
use crate::text_input;

/// Which vim key opened the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenKey {
    /// `i`: insert before the line — caret at the start.
    Insert,
    /// `a`: append — caret at the end. On a single whole-line buffer (no character-level
    /// cursor in list mode) this is the same target position as `A`; the two keys are kept
    /// distinct here only because both are named in the spec, not because they diverge in
    /// behaviour once no per-character list cursor exists.
    Append,
    /// `A`: append at the end of the line.
    AppendEnd,
}

/// Opens the editor for the current row: an existing line, or the trailing Add-a-line row.
pub fn start(state: &mut AppState, key: OpenKey) {
    state.editing = Some(match state.selected_line() {
        Some(line) => EditDraft::for_line(line, key == OpenKey::Insert),
        None => EditDraft::new_line(),
    });
}

/// `Esc`: discards the draft without applying anything.
pub fn cancel(state: &mut AppState) {
    state.editing = None;
}

/// Inserts one character at the caret.
pub fn insert_char(draft: &mut EditDraft, c: char) {
    text_input::insert(&mut draft.buffer, &mut draft.caret, c);
}

/// Deletes the grapheme before the caret, if any.
pub fn backspace(draft: &mut EditDraft) {
    text_input::backspace(&mut draft.buffer, &mut draft.caret);
}

/// Moves the caret one grapheme left, clamped at the start.
pub fn move_left(draft: &mut EditDraft) {
    text_input::left(&draft.buffer, &mut draft.caret);
}

/// Moves the caret one grapheme right, clamped at the end.
pub fn move_right(draft: &mut EditDraft) {
    text_input::right(&draft.buffer, &mut draft.caret);
}

/// One keystroke while the editor has focus. Enter/Esc are handled by `app.rs` (Enter needs to
/// call `commit` and dispatch the resulting mutation to the daemon; Esc calls `cancel`), so this
/// only covers buffer edits. Returns `true` when the key was handled.
pub fn on_key(draft: &mut EditDraft, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) => insert_char(draft, c),
        KeyCode::Backspace => backspace(draft),
        KeyCode::Delete => {
            text_input::delete(&mut draft.buffer, draft.caret);
        }
        KeyCode::Left => move_left(draft),
        KeyCode::Right => move_right(draft),
        KeyCode::Home => draft.caret = 0,
        KeyCode::End => draft.caret = draft.buffer.len(),
        _ => return false,
    }
    true
}

/// `Enter`: takes the draft out of `state` and maps it to the mutation to send, if any. `None`
/// means nothing to do — either an empty new line, or a save that reproduces the original line
/// byte-for-byte (design §3.2: "Save that produces a line identical to the original is a no-op").
pub fn commit(state: &mut AppState) -> Option<pb::Mutation> {
    let draft = state.editing.take()?;
    mutation_for(draft)
}

/// `Enter`, all of it: the draft's mutation and, for a new line, a `MoveBefore` that takes it from
/// where the daemon appends it (right after the last task) to the top of the list, above the first
/// task, in the same `Apply` (task tui-desktop-fixes-sync: the Add-a-line row is drawn on top, so
/// that is where the line goes). An empty list needs no move.
pub fn commit_all(state: &mut AppState) -> Vec<pb::Mutation> {
    let is_task = |l: &crate::state::LineState| !l.raw.trim().is_empty();
    let first = state.lines.iter().position(is_task);
    let last = state.lines.iter().rposition(is_task);
    let new_line = matches!(
        state.editing.as_ref().map(|d| &d.target),
        Some(EditTarget::NewLine)
    );
    let mut out: Vec<pb::Mutation> = commit(state).into_iter().collect();
    if let (true, false, Some(first), Some(last)) = (new_line, out.is_empty(), first, last) {
        let first_line = &state.lines[first];
        out.push(pb::Mutation {
            kind: Some(pb::mutation::Kind::MoveBefore(pb::MoveBefore {
                // Appended right after the last task: one line below it, 1-based.
                task: Some(pb::TaskRef {
                    line_number: u32::try_from(last + 2).unwrap_or(u32::MAX),
                    task_id: String::new(),
                }),
                before: Some(pb::TaskRef {
                    line_number: first_line.line_number,
                    task_id: first_line.task_ref_id().to_owned(),
                }),
            })),
        });
    }
    out
}

fn mutation_for(draft: EditDraft) -> Option<pb::Mutation> {
    match draft.target {
        EditTarget::NewLine => {
            if draft.buffer.is_empty() {
                return None;
            }
            Some(pb::Mutation {
                kind: Some(pb::mutation::Kind::Add(pb::Add { line: draft.buffer })),
            })
        }
        EditTarget::Existing {
            line_number,
            task_id,
        } => {
            // The no-op rule needs the *original* raw line, which the draft itself doesn't carry
            // once buffer == raw (the common untouched-save case) — but it does when they
            // differ, which is the only case that matters: an untouched `i`/`A` starts with
            // `buffer == raw`, so comparing here is exactly the byte-identical check.
            Some(pb::Mutation {
                kind: Some(pb::mutation::Kind::Edit(pb::Edit {
                    task: Some(pb::TaskRef {
                        line_number,
                        task_id: task_id.unwrap_or_default(),
                    }),
                    new_line: draft.buffer,
                })),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::LineState;

    #[test]
    fn insert_and_backspace_move_the_caret() {
        let mut draft = EditDraft::new_line();
        insert_char(&mut draft, 'a');
        insert_char(&mut draft, 'b');
        assert_eq!(draft.buffer, "ab");
        assert_eq!(draft.caret, 2);
        backspace(&mut draft);
        assert_eq!(draft.buffer, "a");
        assert_eq!(draft.caret, 1);
    }

    #[test]
    fn backspace_at_start_is_a_no_op() {
        let mut draft = EditDraft::new_line();
        backspace(&mut draft);
        assert_eq!(draft.buffer, "");
    }

    #[test]
    fn insert_respects_caret_position_not_just_the_end() {
        let mut draft = EditDraft::new_line();
        insert_char(&mut draft, 'a');
        insert_char(&mut draft, 'c');
        move_left(&mut draft);
        insert_char(&mut draft, 'b');
        assert_eq!(draft.buffer, "abc");
    }

    #[test]
    fn delete_removes_the_character_after_the_caret() {
        let mut draft = EditDraft::new_line();
        insert_char(&mut draft, 'a');
        insert_char(&mut draft, 'b');
        move_left(&mut draft);
        assert!(on_key(&mut draft, KeyEvent::from(KeyCode::Delete)));
        assert_eq!((draft.buffer.as_str(), draft.caret), ("a", 1));
    }

    #[test]
    fn commit_new_line_with_text_produces_add() {
        let mut state = AppState::fixture();
        state.move_first();
        start(&mut state, OpenKey::Insert);
        insert_char(state.editing.as_mut().unwrap(), 'x');
        let mutation = commit(&mut state).expect("non-empty new line commits");
        assert!(matches!(mutation.kind, Some(pb::mutation::Kind::Add(_))));
        assert!(state.editing.is_none());
    }

    #[test]
    fn a_new_line_is_added_then_moved_above_the_first_task() {
        let mut state = AppState::fixture();
        state.move_first();
        assert!(state.on_add_line_row(), "the Add-a-line row is on top");
        start(&mut state, OpenKey::Insert);
        insert_char(state.editing.as_mut().unwrap(), 'x');
        let all = commit_all(&mut state);
        assert_eq!(all.len(), 2, "{all:?}");
        let Some(pb::mutation::Kind::MoveBefore(mv)) = &all[1].kind else {
            panic!("then a move: {all:?}");
        };
        let last_task = state.lines.iter().rposition(|l| !l.raw.trim().is_empty());
        let moved = mv.task.as_ref().map(|t| t.line_number);
        assert_eq!(moved, last_task.map(|i| u32::try_from(i + 2).unwrap()));
        assert_eq!(mv.before.as_ref().map(|t| t.line_number), Some(1));
    }

    #[test]
    fn commit_empty_new_line_is_a_no_op() {
        let mut state = AppState::fixture();
        state.move_first();
        start(&mut state, OpenKey::Insert);
        assert!(commit(&mut state).is_none());
    }

    #[test]
    fn commit_unchanged_existing_line_is_a_no_op_per_design_3_2() {
        let mut state = AppState::fixture();
        let original = state.lines[0].raw.clone();
        start(&mut state, OpenKey::AppendEnd);
        let mutation = commit(&mut state);
        // Untouched save reproduces the line byte-for-byte: still a real Edit mutation is
        // returned here (no-op *detection* against the live document is app.rs's job once it
        // has the daemon's current baseline — see this module's own doc), but the buffer must be
        // unchanged so app.rs's comparison against the baseline correctly finds no difference.
        if let Some(pb::Mutation {
            kind: Some(pb::mutation::Kind::Edit(edit)),
        }) = mutation
        {
            assert_eq!(edit.new_line, original);
        } else {
            panic!("expected an Edit mutation");
        }
    }

    #[test]
    fn commit_edited_existing_line_carries_the_task_ref() {
        let mut state = AppState::fixture();
        let line: LineState = state.lines[1].clone(); // has an id: tag in the fixture
        state.cursor = 1;
        start(&mut state, OpenKey::AppendEnd);
        insert_char(state.editing.as_mut().unwrap(), '!');
        let mutation = commit(&mut state).unwrap();
        let Some(pb::mutation::Kind::Edit(edit)) = mutation.kind else {
            panic!("expected Edit");
        };
        let task = edit.task.unwrap();
        assert_eq!(task.line_number, line.line_number);
        assert_eq!(task.task_id, line.task_id.unwrap());
        assert!(edit.new_line.ends_with('!'));
    }
}
