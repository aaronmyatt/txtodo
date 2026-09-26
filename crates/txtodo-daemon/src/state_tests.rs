//! DocState: from_file validation, byte-faithful materialise, every OpKind applied and refused.

use crate::state::{DocState, Entry, StateError, TaskCounts, task_id};
use txtodo_core::{Date, parse_file};
use txtodo_model::{Field, FieldValue, FilePath, OpKind, TextEdit, set_field};

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";
const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAC";

fn ulid_bits(text: &str) -> u128 {
    txtodo_model::Ulid::parse(text).unwrap().to_u128()
}

fn doc(bytes: &[u8]) -> DocState {
    DocState::from_tagged_file(FilePath::new("todo.txt").unwrap(), &parse_file(bytes)).unwrap()
}

fn two_lines_crlf() -> Vec<u8> {
    format!("(A) 2026-09-11 buy ducks +farm id:{A}\r\n\r\nx 2026-09-11 2026-09-10 call mum @phone id:{B}\r\n").into_bytes()
}

#[test]
fn from_file_is_byte_faithful_and_requires_ids() {
    let bytes = two_lines_crlf();
    let state = doc(&bytes);
    assert_eq!(state.to_bytes(), bytes);
    assert_eq!(state.len(), 3);
    assert!(matches!(state.entry_at(1), Some(Entry::Blank(_))));
    assert_eq!(state.index_of(task_id(ulid_bits(B))), Some(2));
    let no_id = parse_file(b"(A) no id here\n");
    assert_eq!(
        DocState::from_tagged_file(FilePath::new("todo.txt").unwrap(), &no_id).unwrap_err(),
        StateError::MissingId(0)
    );
    let opaque = parse_file(b"\xff\xfe bad\n");
    assert_eq!(
        DocState::from_tagged_file(FilePath::new("t.txt").unwrap(), &opaque).unwrap_err(),
        StateError::Opaque(0)
    );
}

#[test]
fn accessors_answer_by_index_and_by_id() {
    let state = doc(&two_lines_crlf());
    let (a, b) = (task_id(ulid_bits(A)), task_id(ulid_bits(B)));
    assert!(!state.is_empty());
    assert_eq!(state.entry_at(3), None, "past the end");
    assert_eq!(state.entry_at(2).and_then(|e| e.id()), Some(b));
    assert_eq!(state.task_before(0), None, "nothing before the first line");
    assert_eq!(state.task_before(2), Some(a), "skips the blank at 1");
    assert_eq!(
        state.task_before(3),
        Some(b),
        "len() asks for the last task"
    );
    let line = state.line_of(a).and_then(|l| l.raw().map(str::to_owned));
    assert_eq!(line, Some(format!("(A) 2026-09-11 buy ducks +farm id:{A}")));
    assert_eq!(state.line_of(task_id(ulid_bits(C))), None);
}

#[test]
fn insert_move_and_blank_ops_reorder_the_document() {
    let mut state = doc(&two_lines_crlf());
    let (a, b, c) = (
        task_id(ulid_bits(A)),
        task_id(ulid_bits(B)),
        task_id(ulid_bits(C)),
    );
    state
        .apply_kind(&OpKind::Insert {
            task: c,
            after: Some(a),
            line: format!("new one id:{C}"),
        })
        .unwrap();
    state
        .apply_kind(&OpKind::BlankRemove { after: Some(c) })
        .unwrap();
    state
        .apply_kind(&OpKind::Move {
            task: b,
            after: None,
            to_file: FilePath::new("todo.txt").unwrap(),
        })
        .unwrap();
    state
        .apply_kind(&OpKind::BlankInsert { after: Some(b) })
        .unwrap();
    let expected = format!(
        "x 2026-09-11 2026-09-10 call mum @phone id:{B}\r\n\r\n(A) 2026-09-11 buy ducks +farm id:{A}\r\nnew one id:{C}\r\n"
    );
    assert_eq!(
        String::from_utf8(state.to_bytes()).unwrap(),
        expected,
        "CRLF kept for inserted lines too"
    );
    let bad = OpKind::Insert {
        task: c,
        after: None,
        line: "wrong id:01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
    };
    assert_eq!(state.apply_kind(&bad), Err(StateError::IdMismatch(c)));
    assert_eq!(
        state.apply_kind(&OpKind::BlankRemove { after: Some(a) }),
        Err(StateError::NoBlank(Some(a)))
    );
}

/// A last line with no final newline: whatever lands after it (an add, the move `do` makes to the
/// bottom, a blank) starts a line of its own, CRLF kept, never glued onto it. Take it away again
/// and the bytes are the original ones.
#[test]
fn ops_after_a_last_line_with_no_newline_start_a_line_of_their_own() {
    let before = format!("(A) buy ducks id:{A}\r\nwalk the dog id:{B}");
    let mut state = doc(before.as_bytes());
    let (a, b, c) = (
        task_id(ulid_bits(A)),
        task_id(ulid_bits(B)),
        task_id(ulid_bits(C)),
    );
    let text = |state: &DocState| String::from_utf8(state.to_bytes()).unwrap();
    let insert = OpKind::Insert {
        task: c,
        after: Some(b),
        line: format!("call mum id:{C}"),
    };
    state.apply_kind(&insert).unwrap();
    assert_eq!(text(&state), format!("{before}\r\ncall mum id:{C}\r\n"));
    let delete = set_field(c, Field::Deleted, FieldValue::Bool(true)).unwrap();
    state.apply_kind(&delete).unwrap();
    assert_eq!(text(&state), before, "the add undone: bytes as they were");

    let here = FilePath::new("todo.txt").unwrap();
    let to_bottom = |task, after| OpKind::Move {
        task,
        after: Some(after),
        to_file: here.clone(),
    };
    state.apply_kind(&to_bottom(a, b)).unwrap();
    assert_eq!(
        text(&state),
        format!("walk the dog id:{B}\r\n(A) buy ducks id:{A}\r\n")
    );
    state.apply_kind(&to_bottom(b, a)).unwrap();
    assert_eq!(text(&state), before, "last again: no final newline again");

    state
        .apply_kind(&OpKind::BlankInsert { after: Some(b) })
        .unwrap();
    assert_eq!(
        text(&state),
        format!("{before}\r\n\r\n"),
        "the blank is kept"
    );
    assert_eq!(parse_file(&state.to_bytes()).lines.len(), state.len());
}

// Cross-file: this document is the source, so the task just leaves. `after` names a position in
// `to_file`, meaningless here — the destination actor never replays this op (see `move_task`'s
// doc and `move_coordinator_tests`).
#[test]
fn cross_file_move_only_removes_on_the_source() {
    let mut state = doc(&two_lines_crlf());
    let a = task_id(ulid_bits(A));
    let other = FilePath::new("other.txt").unwrap();
    assert!(state.index_of(a).is_some());
    state
        .apply_kind(&OpKind::Move {
            task: a,
            after: None,
            to_file: other,
        })
        .unwrap();
    assert_eq!(state.index_of(a), None, "the task left this document");
    assert_eq!(
        state.apply_kind(&OpKind::Move {
            task: a,
            after: None,
            to_file: FilePath::new("todo.txt").unwrap(),
        }),
        Err(StateError::UnknownTask(a)),
        "it is gone, so a second move of it fails"
    );
}

#[test]
fn set_field_rewrites_the_prefix_and_delete_removes_the_line() {
    let mut state = doc(&two_lines_crlf());
    let a = task_id(ulid_bits(A));
    state
        .apply_kind(&set_field(a, Field::Priority, FieldValue::Priority(Some('B'))).unwrap())
        .unwrap();
    assert!(state.to_bytes().starts_with(b"(B) 2026-09-11 buy ducks"));
    state
        .apply_kind(&set_field(a, Field::CreationDate, FieldValue::Date(None)).unwrap())
        .unwrap();
    assert!(state.to_bytes().starts_with(b"(B) buy ducks +farm"));
    // Completing keeps the priority as pri:, like core's Edit::complete.
    state
        .apply_kind(&set_field(a, Field::Completed, FieldValue::Bool(true)).unwrap())
        .unwrap();
    state
        .apply_kind(
            &set_field(
                a,
                Field::CompletionDate,
                FieldValue::date(Date::new(2026, 9, 12)),
            )
            .unwrap(),
        )
        .unwrap();
    let first = String::from_utf8(state.to_bytes())
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(first, format!("x 2026-09-12 buy ducks +farm id:{A} pri:B"));
    state
        .apply_kind(&set_field(a, Field::Deleted, FieldValue::Bool(true)).unwrap())
        .unwrap();
    assert_eq!(state.len(), 2);
    assert_eq!(state.line_of(a), None);
    assert_eq!(
        state.apply_kind(&set_field(a, Field::Deleted, FieldValue::Bool(true)).unwrap()),
        Err(StateError::UnknownTask(a))
    );
}

#[test]
fn task_counts_excludes_blanks_and_counts_completed() {
    // Two done, one undone, one blank (plan §3.2.5's rule: blanks excluded from both counts).
    let bytes = format!(
        "(A) 2026-09-11 buy ducks +farm id:{A}\n\nwalk the dog @home id:{B}\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
    );
    let state = doc(bytes.as_bytes());
    assert_eq!(
        state.task_counts(),
        TaskCounts {
            total: 3,
            completed: 1
        }
    );
}

#[test]
fn task_counts_is_zero_for_an_empty_document() {
    let state = doc(b"");
    assert_eq!(state.task_counts(), TaskCounts::default());
}

#[test]
fn edit_text_changes_only_the_description() {
    let mut state = doc(&two_lines_crlf());
    let a = task_id(ulid_bits(A));
    // "buy ducks +farm id:A" → "buy 400 ducks +farm id:A": insert "400 " at target char 4.
    let edits = vec![TextEdit::Insert {
        at: 4,
        text: "400 ".into(),
    }];
    state
        .apply_kind(&OpKind::EditText { task: a, edits })
        .unwrap();
    assert!(
        state
            .to_bytes()
            .starts_with(format!("(A) 2026-09-11 buy 400 ducks +farm id:{A}\r\n").as_bytes())
    );
    let strip_id = vec![TextEdit::Delete { at: 0, len: 60 }];
    assert!(matches!(
        state.apply_kind(&OpKind::EditText {
            task: a,
            edits: strip_id
        }),
        Err(StateError::Text(..))
    ));
    let remove_tag = vec![TextEdit::Delete { at: 20, len: 29 }];
    assert_eq!(
        state.apply_kind(&OpKind::EditText {
            task: a,
            edits: remove_tag
        }),
        Err(StateError::IdMismatch(a))
    );
}
