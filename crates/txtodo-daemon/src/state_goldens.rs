//! Characterization goldens for `DocState::to_bytes` (constitution §4: pin behaviour before
//! changing it). Each test replays one of plan M3's eight external-edit scenarios through the pure
//! `reconcile` and `DocState::apply`, then asserts the exact bytes. The M4 Loro swap
//! (`tasks/crdt-loro-state`) must leave every literal here untouched.

use crate::reconcile::reconcile;
use crate::state::{DocState, task_id};
use txtodo_core::parse_file;
use txtodo_model::{FilePath, OpKind, TaskId};

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";
const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAC";
/// Minted ids start here; `mint()` counts up so each golden is deterministic.
const MINT_BASE: u128 = 0x1000;

fn todo_with_ids() -> String {
    format!(
        "(A) 2026-09-11 buy ducks +farm id:{A}\n\nwalk the dog @home id:{B}\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
    )
}

fn path() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

/// Outcome of one replayed scenario.
struct Replayed {
    ops: Vec<OpKind>,
    /// `DocState::to_bytes()` after every op.
    bytes: String,
    /// The reconciled file's bytes; when these differ from `bytes` the actor adopts the file.
    reconciled: String,
    /// Ids minted, in order, as their `id:` text.
    minted: Vec<String>,
}

/// Builds the state from `before`, reconciles against `after`, applies every op, returns bytes.
fn replay(before: &str, after: &str) -> Replayed {
    let old = parse_file(before.as_bytes());
    let new = parse_file(after.as_bytes());
    let mut state = DocState::from_tagged_file(path(), &old).unwrap();
    assert_eq!(
        state.to_bytes(),
        before.as_bytes(),
        "from_file is byte-faithful"
    );
    let mut next = MINT_BASE;
    let mut minted = Vec::new();
    let mut mint = || -> TaskId {
        let id = task_id(next);
        next += 1;
        minted.push(id.ulid().to_string());
        id
    };
    let reconciled = reconcile(&old, &new, &path(), &mut mint);
    for op in &reconciled.ops {
        state.apply_kind(op).unwrap();
    }
    assert!(
        reconciled.ops.len() <= 6 * new.lines.len() + old.lines.len(),
        "op count is bounded by the line counts"
    );
    Replayed {
        ops: reconciled.ops,
        bytes: String::from_utf8(state.to_bytes()).unwrap(),
        reconciled: String::from_utf8(reconciled.file.to_bytes()).unwrap(),
        minted,
    }
}

fn kinds(ops: &[OpKind]) -> Vec<&'static str> {
    ops.iter()
        .map(|op| match op {
            OpKind::Insert { .. } => "insert",
            OpKind::SetField { .. } => "set_field",
            OpKind::EditText { .. } => "edit_text",
            OpKind::Move { .. } => "move",
            OpKind::BlankInsert { .. } => "blank_insert",
            OpKind::BlankRemove { .. } => "blank_remove",
            OpKind::NotesEdit { .. } => "notes_edit",
        })
        .collect()
}

#[test]
fn golden_edit_description_in_place() {
    let before = todo_with_ids();
    let after = before.replacen("buy ducks", "buy 400 ducks", 1);
    let r = replay(&before, &after);
    assert_eq!(kinds(&r.ops), vec!["edit_text"]);
    assert_eq!(
        r.bytes,
        format!(
            "(A) 2026-09-11 buy 400 ducks +farm id:{A}\n\nwalk the dog @home id:{B}\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
        )
    );
    assert!(r.minted.is_empty(), "ids were present, nothing minted");
    assert_eq!(r.bytes, r.reconciled, "no adopt needed");
}

#[test]
fn golden_insert_a_line_in_the_middle_gets_an_id() {
    let before = todo_with_ids();
    let mut lines: Vec<&str> = before.lines().collect();
    lines.insert(2, "new one +farm");
    let after = lines.join("\n") + "\n";
    let r = replay(&before, &after);
    assert_eq!(kinds(&r.ops), vec!["insert"]);
    assert_eq!(r.minted.len(), 1);
    let m = &r.minted[0];
    // M3 anchors the insert "after task A", so the state lands it before the blank line while the
    // file has it after: apply(ops) != file and the actor adopts the file (design §4.3 step 6).
    assert_eq!(
        r.bytes,
        format!(
            "(A) 2026-09-11 buy ducks +farm id:{A}\nnew one +farm id:{m}\n\nwalk the dog @home id:{B}\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
        )
    );
    assert_eq!(
        r.reconciled,
        format!(
            "(A) 2026-09-11 buy ducks +farm id:{A}\n\nnew one +farm id:{m}\nwalk the dog @home id:{B}\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
        ),
        "the adopted file keeps the user's placement"
    );
}

#[test]
fn golden_delete_a_line() {
    let before = todo_with_ids();
    let after: String = before
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .map(|(_, l)| format!("{l}\n"))
        .collect();
    let r = replay(&before, &after);
    assert_eq!(kinds(&r.ops), vec!["set_field"], "Deleted = true");
    assert_eq!(
        r.bytes,
        format!(
            "(A) 2026-09-11 buy ducks +farm id:{A}\n\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
        )
    );
    assert_eq!(r.bytes, r.reconciled, "no adopt needed");
}

#[test]
fn golden_reorder_two_lines() {
    let before = todo_with_ids();
    let mut lines: Vec<&str> = before.lines().collect();
    lines.swap(0, 2);
    let after = lines.join("\n") + "\n";
    let r = replay(&before, &after);
    assert!(
        kinds(&r.ops)
            .iter()
            .all(|k| matches!(*k, "move" | "blank_remove" | "blank_insert")),
        "{:?}",
        kinds(&r.ops)
    );
    assert_eq!(
        r.bytes,
        format!(
            "walk the dog @home id:{B}\n\n(A) 2026-09-11 buy ducks +farm id:{A}\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
        )
    );
    assert_eq!(r.bytes, r.reconciled, "no adopt needed");
}

#[test]
fn golden_strip_every_id_restores_them() {
    let before = todo_with_ids();
    let after: String = before
        .lines()
        .map(|l| format!("{}\n", l.split(" id:").next().unwrap_or(l)))
        .collect();
    let r = replay(&before, &after);
    assert!(
        r.ops.is_empty(),
        "recovering ids is not a change: {:?}",
        kinds(&r.ops)
    );
    assert_eq!(
        r.bytes, before,
        "the same ids come back, nothing else moves"
    );
    assert!(r.minted.is_empty());
    assert_eq!(r.bytes, r.reconciled, "no adopt needed");
}

#[test]
fn golden_append_a_line_without_an_id() {
    let before = todo_with_ids();
    let after = before.clone() + "typed in vim\n";
    let r = replay(&before, &after);
    assert_eq!(kinds(&r.ops), vec!["insert"]);
    assert_eq!(r.minted.len(), 1);
    let m = &r.minted[0];
    assert_eq!(r.bytes, format!("{before}typed in vim id:{m}\n"));
    assert_eq!(r.bytes, r.reconciled, "no adopt needed");
}

#[test]
fn golden_same_content_with_crlf_yields_no_ops() {
    let before = todo_with_ids();
    let after = before.replace('\n', "\r\n");
    // Zero ops: the state stays LF and the actor adopts the CRLF file (apply(ops) != file).
    let old = parse_file(before.as_bytes());
    let new = parse_file(after.as_bytes());
    let mut state = DocState::from_tagged_file(path(), &old).unwrap();
    let mut mint = || task_id(MINT_BASE);
    let reconciled = reconcile(&old, &new, &path(), &mut mint);
    assert!(reconciled.ops.is_empty(), "{:?}", kinds(&reconciled.ops));
    for op in &reconciled.ops {
        state.apply_kind(op).unwrap();
    }
    assert_eq!(
        state.to_bytes(),
        before.as_bytes(),
        "the state keeps its ending"
    );
    assert_ne!(
        state.to_bytes(),
        reconciled.file.to_bytes(),
        "so the actor adopts"
    );
}

#[test]
fn golden_todo_sh_do_marks_done_then_archives() {
    // `todo.sh do 3` rewrites line 3 as done (one save), then archive removes it (a second save).
    let before = todo_with_ids();
    let done = before.replacen(
        &format!("walk the dog @home id:{B}"),
        &format!("x 2026-09-12 walk the dog @home id:{B}"),
        1,
    );
    let first = replay(&before, &done);
    assert_eq!(
        kinds(&first.ops),
        vec!["set_field", "set_field"],
        "completed + date"
    );
    assert_eq!(
        first.bytes,
        format!(
            "(A) 2026-09-11 buy ducks +farm id:{A}\n\nx 2026-09-12 walk the dog @home id:{B}\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
        )
    );
    let archived = done.replacen(&format!("x 2026-09-12 walk the dog @home id:{B}\n"), "", 1);
    let second = replay(&done, &archived);
    assert_eq!(kinds(&second.ops), vec!["set_field"], "Deleted = true");
    assert_eq!(first.bytes, first.reconciled, "no adopt needed");
    assert_eq!(second.bytes, second.reconciled, "no adopt needed");
    assert_eq!(
        second.bytes,
        format!(
            "(A) 2026-09-11 buy ducks +farm id:{A}\n\nx 2026-09-11 2026-09-10 call mum @phone id:{C}\n"
        )
    );
}
