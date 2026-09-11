//! Unit tests for the op model: pairing guard, round trips, Display goldens.

use crate::*;
use txtodo_core::{Date, Priority, Quirks, Ulid};

fn task() -> TaskId {
    TaskId::new(Ulid::from_u128(42))
}

fn device() -> DeviceId {
    DeviceId::new(Ulid::from_u128(7))
}

#[test]
fn set_field_rejects_a_mismatched_pair_and_accepts_every_valid_one() {
    assert_eq!(
        set_field(task(), Field::Priority, FieldValue::Bool(true)),
        Err(FieldMismatch {
            field: Field::Priority
        })
    );
    let ok = [
        (Field::Completed, FieldValue::Bool(true)),
        (Field::Deleted, FieldValue::Bool(false)),
        (
            Field::CompletionDate,
            FieldValue::date(Date::new(2026, 9, 11)),
        ),
        (Field::CreationDate, FieldValue::Date(None)),
        (Field::Priority, FieldValue::priority(Priority::new('A'))),
        (Field::Quirks, FieldValue::quirks(Quirks::TABS)),
    ];
    for (field, value) in ok {
        assert!(set_field(task(), field, value).is_ok(), "{field:?}");
    }
}

#[test]
fn field_values_round_trip_core_types() {
    let d = Date::new(2026, 2, 28);
    assert_eq!(FieldValue::date(d).as_date(), Some(d));
    assert_eq!(
        FieldValue::Date(Some((2026, 2, 30))).as_date(),
        None,
        "invalid dates do not parse"
    );
    assert_eq!(FieldValue::Bool(true).as_date(), None);
    let mut q = Quirks::NONE;
    q.insert(Quirks::TABS);
    q.insert(Quirks::LEADING_WS);
    assert_eq!(FieldValue::quirks(q).as_quirks(), Some(q));
    assert_eq!(FieldValue::quirks(Quirks::NONE), FieldValue::Quirks(0));
}

#[test]
fn ops_round_trip_through_postcard() {
    let mut hlc = Hlc::zero(device());
    let op = Op {
        id: OpId::new(Ulid::from_u128(1)),
        hlc: hlc.tick(1_700_000_000_000).unwrap(),
        principal: Principal::External { device: device() },
        file: FilePath::new("q4/todo.txt").unwrap(),
        kind: OpKind::EditText {
            task: task(),
            edits: vec![
                TextEdit::Insert {
                    at: 3,
                    text: "héllo".into(),
                },
                TextEdit::Delete { at: 9, len: 2 },
            ],
        },
    };
    let bytes = postcard::to_allocvec(&op).unwrap();
    assert_eq!(postcard::from_bytes::<Op>(&bytes).unwrap(), op);
    let core_edit: txtodo_core::TextEdit = TextEdit::Delete { at: 1, len: 2 }.into();
    assert_eq!(
        TextEdit::from(core_edit),
        TextEdit::Delete { at: 1, len: 2 }
    );
}

#[test]
fn principal_display_matches_design_4_8() {
    let d = device().to_string();
    assert_eq!(
        Principal::User { device: device() }.to_string(),
        format!("you@{d}")
    );
    let agent = Principal::Agent {
        token_id: TokenId::new(Ulid::from_u128(9)),
        name: "claude-code".into(),
        device: device(),
    };
    assert_eq!(agent.to_string(), format!("agent:claude-code@{d}"));
    assert_eq!(
        Principal::External { device: device() }.to_string(),
        format!("external@{d}")
    );
}
