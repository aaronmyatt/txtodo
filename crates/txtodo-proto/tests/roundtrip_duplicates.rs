//! ADR 0032's duplicate groups (task duplicate-flags) encode and decode to themselves, and a
//! `ConflictsResponse` from an older daemon reads as no duplicates. Split from `roundtrip.rs` for
//! its file budget.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use prost::Message;
use txtodo_proto::v1::{ConflictsResponse, DuplicateGroup, DuplicateTask};

#[test]
fn a_conflicts_response_with_duplicates_round_trips() {
    let resp = ConflictsResponse {
        flags: Vec::new(),
        duplicates: vec![DuplicateGroup {
            tasks: vec![
                DuplicateTask {
                    task_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
                    line_number: 4,
                },
                DuplicateTask {
                    task_id: "01M3AAAAAAAAAAAAAAAAAAAAAA".into(),
                    line_number: 1,
                },
            ],
        }],
    };
    let back = ConflictsResponse::decode(resp.encode_to_vec().as_slice()).unwrap();
    assert_eq!(back, resp);
}

#[test]
fn an_older_daemons_conflicts_response_has_no_duplicates() {
    // Field 1 only, as a daemon from before ADR 0032 sends it: an empty flag list.
    let old = ConflictsResponse::default().encode_to_vec();
    assert!(
        ConflictsResponse::decode(old.as_slice())
            .unwrap()
            .duplicates
            .is_empty()
    );
}
