//! The host-facing views: fork is independent, list_ids keeps order and sentinels, last_blank_id
//! follows BlankInsert, and re-inserting a deleted id resurrects one entry with a reset text.

use txtodo_model::{
    DeviceId, Field, FieldValue, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, Ulid,
    set_field,
};

use crate::{HydrateLine, LoroDocument, apply, hydrate_file, is_blank, rebuild_line};

fn dev() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

fn hlc(n: u64) -> Hlc {
    Hlc {
        wall_ms: n,
        counter: 0,
        device: dev(),
    }
}

fn task(n: u128) -> TaskId {
    TaskId::new(Ulid::from_u128(n))
}

fn file() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn op(n: u64, kind: OpKind) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(u128::from(n))),
        hlc: hlc(n),
        principal: Principal::External { device: dev() },
        file: file(),
        kind,
    }
}

fn insert(n: u64, t: u128, after: Option<TaskId>, text: &str) -> Op {
    op(
        n,
        OpKind::Insert {
            task: task(t),
            after,
            line: format!("{text} id:{}", Ulid::from_u128(t)),
        },
    )
}

/// A doc with tasks 1 and 2 and a blank between them.
fn two_tasks_and_a_blank() -> LoroDocument {
    let mut doc = LoroDocument::open();
    apply(&mut doc, &insert(1, 1, None, "first")).unwrap();
    apply(&mut doc, &insert(2, 2, Some(task(1)), "second")).unwrap();
    apply(
        &mut doc,
        &op(
            3,
            OpKind::BlankInsert {
                after: Some(task(1)),
            },
        ),
    )
    .unwrap();
    doc
}

#[test]
fn list_ids_keeps_order_and_sentinels_and_last_blank_id_tracks_blank_insert() {
    let doc = two_tasks_and_a_blank();
    let ids = doc.list_ids(&file());
    assert_eq!(ids.len(), 3);
    assert_eq!((ids[0], ids[2]), (task(1), task(2)));
    assert!(
        is_blank(ids[1]),
        "the sentinel sits where the blank was inserted"
    );
    assert_eq!(doc.last_blank_id(), Some(ids[1]));
    assert_eq!(LoroDocument::open().last_blank_id(), None);
    assert!(doc.list_ids(&FilePath::new("done.txt").unwrap()).is_empty());
}

#[test]
fn fork_is_a_deep_copy_that_the_original_never_sees() {
    let doc = two_tasks_and_a_blank();
    let mut forked = doc.fork();
    apply(&mut forked, &insert(4, 3, Some(task(2)), "third")).unwrap();
    apply(&mut forked, &op(5, OpKind::BlankInsert { after: None })).unwrap();
    assert_eq!(forked.list_ids(&file()).len(), 5);
    assert_eq!(doc.list_ids(&file()).len(), 3, "the original is untouched");
    assert_ne!(forked.last_blank_id(), doc.last_blank_id());
    assert_eq!(doc.fork().list_ids(&file()), doc.list_ids(&file()));
}

#[test]
fn reinserting_a_deleted_id_resurrects_one_entry_with_a_reset_description() {
    let mut doc = two_tasks_and_a_blank();
    let deleted = op(
        4,
        set_field(task(2), Field::Deleted, FieldValue::Bool(true)).unwrap(),
    );
    apply(&mut doc, &deleted).unwrap();
    assert_eq!(
        doc.list_ids(&file()).len(),
        3,
        "delete is a flag, the entry stays"
    );
    // Undo replays the original Insert with the same id, at the front this time.
    apply(&mut doc, &insert(5, 2, None, "second again")).unwrap();
    let ids = doc.list_ids(&file());
    assert_eq!(ids.len(), 3, "one entry per id, never a duplicate");
    assert_eq!(ids[0], task(2), "moved to the new position");
    let line = rebuild_line(&doc, task(2)).unwrap();
    assert!(line.starts_with("second again id:"), "{line}");
    assert!(
        !line.contains("secondsecond"),
        "the stale text was reset: {line}"
    );
}

#[test]
fn blank_remove_skips_a_deleted_tombstone_and_set_description_replaces_the_text() {
    // A, X, blank, B — then delete X; BlankRemove after A must take the blank, not X's tombstone.
    let mut doc = LoroDocument::open();
    apply(&mut doc, &insert(1, 1, None, "a")).unwrap();
    apply(&mut doc, &insert(2, 9, Some(task(1)), "x")).unwrap();
    apply(
        &mut doc,
        &op(
            3,
            OpKind::BlankInsert {
                after: Some(task(9)),
            },
        ),
    )
    .unwrap();
    let blank = doc.last_blank_id().unwrap();
    apply(&mut doc, &insert(4, 2, Some(blank), "b")).unwrap();
    apply(
        &mut doc,
        &op(
            5,
            set_field(task(9), Field::Deleted, FieldValue::Bool(true)).unwrap(),
        ),
    )
    .unwrap();
    assert!(doc.is_deleted(task(9)) && !doc.is_deleted(task(1)));
    apply(
        &mut doc,
        &op(
            6,
            OpKind::BlankRemove {
                after: Some(task(1)),
            },
        ),
    )
    .unwrap();
    let ids = doc.list_ids(&file());
    assert_eq!(
        ids,
        vec![task(1), task(9), task(2)],
        "blank gone, tombstone kept"
    );
    let again = apply(
        &mut doc,
        &op(
            7,
            OpKind::BlankRemove {
                after: Some(task(1)),
            },
        ),
    );
    assert!(matches!(again, Err(crate::ToLoroError::NoBlankAfter(_))));
    doc.set_description(task(2), "b pri:B").unwrap();
    assert_eq!(doc.description(task(2)).as_deref(), Some("b pri:B"));
    assert_eq!(doc.description(task(3)), None);
}

#[test]
fn hydrate_file_appends_in_order_under_one_commit_and_refuses_a_non_empty_list() {
    let mut doc = LoroDocument::open();
    let a = format!("a id:{}", Ulid::from_u128(1));
    let b = format!("b id:{}", Ulid::from_u128(2));
    let lines = [
        HydrateLine::Task {
            task: task(1),
            line: &a,
        },
        HydrateLine::Blank,
        HydrateLine::Blank,
        HydrateLine::Task {
            task: task(2),
            line: &b,
        },
    ];
    let ids = hydrate_file(&mut doc, &file(), &lines, hlc(0)).unwrap();
    assert_eq!(ids.len(), 4);
    assert_eq!((ids[0], ids[3]), (task(1), task(2)));
    assert!(is_blank(ids[1]) && is_blank(ids[2]) && ids[1] != ids[2]);
    assert_eq!(
        doc.list_ids(&file()),
        ids,
        "the list is exactly the hydrated order"
    );
    assert_eq!(doc.description(task(2)).as_deref(), Some(b.as_str()));
    assert!(matches!(
        hydrate_file(&mut doc, &file(), &lines, hlc(0)),
        Err(crate::ToLoroError::Unsupported(_))
    ));
}
