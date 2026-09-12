//! Two documents that share lineage edit the same task offline, then one imports the other:
//! the same word twice → exactly one flag; different words → none; the same word in sequence
//! (the second side had already seen the first) → none; an import of nothing → nothing.

use txtodo_model::{
    DeviceId, Field, FieldValue, FilePath, Hlc, Op, OpId, OpKind, Principal, TaskId, TextEdit,
    Ulid, set_field,
};

use crate::{LoroDocument, apply, detect};

const DESC: &str = "buy ducks +farm";

fn dev(n: u128) -> DeviceId {
    DeviceId::new(Ulid::from_u128(n))
}

fn task() -> TaskId {
    TaskId::new(Ulid::from_u128(0x77))
}

fn file() -> FilePath {
    FilePath::new("todo.txt").unwrap()
}

fn op(device: u128, n: u64, kind: OpKind) -> Op {
    Op {
        id: OpId::new(Ulid::from_u128(u128::from(n) + device * 1000)),
        hlc: Hlc {
            wall_ms: n,
            counter: 0,
            device: dev(device),
        },
        principal: Principal::External {
            device: dev(device),
        },
        file: file(),
        kind,
    }
}

/// `word` (a substring of the current description) replaced by `with`, as an EditText.
fn replace(device: u128, n: u64, current: &str, word: &str, with: &str) -> Op {
    let at = current.find(word).unwrap();
    op(
        device,
        n,
        OpKind::EditText {
            task: task(),
            edits: vec![
                TextEdit::Delete {
                    at,
                    len: word.chars().count(),
                },
                TextEdit::Insert {
                    at,
                    text: with.into(),
                },
            ],
        },
    )
}

/// Device 1's document with one task, and device 2's fork of it (shared lineage, own peer id).
fn paired() -> (LoroDocument, LoroDocument) {
    let mut a = LoroDocument::open();
    a.set_peer(1).unwrap();
    apply(
        &mut a,
        &op(
            1,
            1,
            OpKind::Insert {
                task: task(),
                after: None,
                line: format!("{DESC} id:{}", Ulid::from_u128(0x77)),
            },
        ),
    )
    .unwrap();
    let b = a.fork();
    b.set_peer(2).unwrap();
    (a, b)
}

/// Exports everything `to` lacks from `from` and imports it.
fn sync(from: &LoroDocument, to: &mut LoroDocument) -> crate::Imported {
    let bytes = from.export_updates(&to.version()).unwrap();
    to.import(&bytes).unwrap()
}

#[test]
fn the_same_word_edited_on_both_sides_raises_exactly_one_flag_with_both_texts() {
    let (mut a, mut b) = paired();
    apply(&mut a, &replace(1, 10, DESC, "ducks", "geese")).unwrap();
    apply(&mut b, &replace(2, 11, DESC, "ducks", "cows")).unwrap();
    let imported = sync(&b, &mut a);
    assert!(imported.applied);
    let review = detect(&a, &imported).unwrap();
    assert_eq!(review.flags.len(), 1, "{review:?}");
    assert!(review.overflowed.is_empty());
    let flag = &review.flags[0];
    assert_eq!((flag.task, &flag.file), (task(), &file()));
    assert_eq!(flag.mine, "buy geese +farm id:0000000000000000000000003Q");
    assert_eq!(flag.theirs, "buy cows +farm id:0000000000000000000000003Q");
    // The merged text is whatever Loro made of it; it is not stored and not asserted here.
    assert!(a.description(task()).is_some());
}

#[test]
fn different_words_on_each_side_merge_cleanly_without_a_flag() {
    let (mut a, mut b) = paired();
    apply(&mut a, &replace(1, 10, DESC, "ducks", "geese")).unwrap();
    apply(&mut b, &replace(2, 11, DESC, "farm", "barn")).unwrap();
    let imported = sync(&b, &mut a);
    let review = detect(&a, &imported).unwrap();
    assert!(review.flags.is_empty(), "{review:?}");
    let merged = a.description(task()).unwrap();
    assert!(
        merged.starts_with("buy geese +barn"),
        "both edits survive: {merged}"
    );
}

#[test]
fn a_sequential_edit_of_the_same_word_is_not_a_conflict() {
    let (mut a, mut b) = paired();
    apply(&mut a, &replace(1, 10, DESC, "ducks", "geese")).unwrap();
    let first = sync(&a, &mut b);
    assert!(first.applied);
    assert!(detect(&b, &first).unwrap().flags.is_empty());
    // b now sees "geese" and rewrites that same word: causally after a's edit.
    let current = b.description(task()).unwrap();
    apply(&mut b, &replace(2, 20, &current, "geese", "cows")).unwrap();
    let back = sync(&b, &mut a);
    let review = detect(&a, &back).unwrap();
    assert!(review.flags.is_empty(), "{review:?}");
    assert!(a.description(task()).unwrap().starts_with("buy cows"));
}

#[test]
fn field_only_changes_and_empty_imports_never_flag() {
    let (mut a, mut b) = paired();
    apply(&mut a, &replace(1, 10, DESC, "ducks", "geese")).unwrap();
    apply(
        &mut b,
        &op(
            2,
            11,
            set_field(task(), Field::Priority, FieldValue::Priority(Some('B'))).unwrap(),
        ),
    )
    .unwrap();
    let imported = sync(&b, &mut a);
    assert!(imported.applied);
    assert!(detect(&a, &imported).unwrap().flags.is_empty());
    let nothing = sync(&b, &mut a);
    assert!(!nothing.applied, "nothing new the second time");
    assert_eq!(detect(&a, &nothing).unwrap(), crate::Review::default());
}
