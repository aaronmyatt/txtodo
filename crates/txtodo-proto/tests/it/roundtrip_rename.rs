//! The vanity-name messages (task workspace-vanity-name) encode and decode to themselves, and a
//! `WorkspaceInfo` from an older daemon reads as no name. Split from `roundtrip.rs` for its file
//! budget.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use prost::Message;
use txtodo_proto::v1::{WorkspaceInfo, WorkspaceRenameRequest};

fn round_trip<M: Message + Default + PartialEq + std::fmt::Debug>(m: &M) {
    let bytes = m.encode_to_vec();
    let back = M::decode(bytes.as_slice()).unwrap();
    assert_eq!(&back, m);
}

#[test]
fn the_rename_request_and_a_named_workspace_round_trip() {
    round_trip(&WorkspaceRenameRequest {
        workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
        name: "Groceries \u{1f955}".into(),
    });
    round_trip(&WorkspaceInfo {
        workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
        root: "/home/a/.local/share/txtodo/remote/01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
        is_remote: true,
        name: "Groceries".into(),
        ..WorkspaceInfo::default()
    });
}

#[test]
fn a_workspace_info_without_the_name_field_reads_as_no_name() {
    // What an older daemon sends: every field it knows, and no field 12.
    let older = WorkspaceInfo {
        workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
        root: "/home/a/todo".into(),
        ..WorkspaceInfo::default()
    };
    let back = WorkspaceInfo::decode(older.encode_to_vec().as_slice()).unwrap();
    assert!(back.name.is_empty(), "{back:?}");
    assert_eq!(back.root, "/home/a/todo");
}
