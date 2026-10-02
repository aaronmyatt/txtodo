//! `OpKind::RemoveTag` on the mirror (ADR 0036), and the exhaustive label match that pins every
//! `OpKind` arm (moved here from `roundtrip_tests.rs` for its file budget).

use std::error::Error;

use txtodo_model::{Field, FieldValue, OpKind};

use crate::apply;
use crate::roundtrip_tests::{capture, hlc, mint, op, task, todo_file, two_tasks};

#[test]
fn remove_tag_drops_the_first_tag_and_reads_back_as_a_text_edit() -> Result<(), Box<dyn Error>> {
    let mut doc = two_tasks()?;
    let c = task(3);
    let insert = OpKind::Insert {
        task: c,
        after: None,
        line: "call pri:B mum pri:C".to_owned(),
    };
    apply(&mut doc, &op(3, hlc(2), todo_file(), insert))?;
    let remove = OpKind::RemoveTag {
        task: c,
        key: "pri".to_owned(),
    };
    let ops = capture(&mut doc, &op(4, hlc(3), todo_file(), remove), &mut mint())?;
    assert_eq!(doc.description(c).as_deref(), Some("call mum pri:C"));
    // A Loro diff has no tag names: a peer importing it gets a plain text edit.
    assert!(
        matches!(&ops[..], [o] if matches!(o.kind, OpKind::EditText { .. })),
        "{ops:?}"
    );
    let none = OpKind::RemoveTag {
        task: c,
        key: "due".to_owned(),
    };
    apply(&mut doc, &op(5, hlc(4), todo_file(), none))?;
    assert_eq!(doc.description(c).as_deref(), Some("call mum pri:C"));
    Ok(())
}

/// The exhaustive label match: adding an `OpKind` variant breaks this at compile time.
fn kind_label(kind: &OpKind) -> &'static str {
    match kind {
        OpKind::Insert { .. } => "insert",
        OpKind::SetField { .. } => "set_field",
        OpKind::EditText { .. } => "edit_text",
        OpKind::Move { .. } => "move",
        OpKind::NotesEdit { .. } => "notes_edit",
        OpKind::BlankInsert { .. } => "blank_insert",
        OpKind::BlankRemove { .. } => "blank_remove",
        OpKind::RemoveTag { .. } => "remove_tag",
    }
}

#[test]
fn every_op_kind_is_labelled() {
    let cases = [
        OpKind::Insert {
            task: task(1),
            after: None,
            line: "x".to_owned(),
        },
        OpKind::SetField {
            task: task(1),
            field: Field::Completed,
            value: FieldValue::Bool(true),
        },
        OpKind::EditText {
            task: task(1),
            edits: vec![],
        },
        OpKind::Move {
            task: task(1),
            after: None,
            to_file: todo_file(),
        },
        OpKind::NotesEdit {
            file: todo_file(),
            edits: vec![],
        },
        OpKind::BlankInsert { after: None },
        OpKind::BlankRemove { after: None },
        OpKind::RemoveTag {
            task: task(1),
            key: "pri".to_owned(),
        },
    ];
    let labels: Vec<&str> = cases.iter().map(kind_label).collect();
    assert_eq!(
        labels,
        [
            "insert",
            "set_field",
            "edit_text",
            "move",
            "notes_edit",
            "blank_insert",
            "blank_remove",
            "remove_tag",
        ]
    );
}
