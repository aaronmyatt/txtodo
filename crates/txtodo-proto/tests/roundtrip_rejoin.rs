//! The rejoin messages (task sync-drift line 8) encode and decode to themselves, lists and the
//! nested `WorkspaceInfo` included. Split from `roundtrip.rs` for its file budget.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use prost::Message;
use txtodo_proto::v1::{WorkspaceInfo, WorkspaceRejoinRequest, WorkspaceRejoinResponse};

fn round_trip<M: Message + Default + PartialEq + std::fmt::Debug>(m: &M) {
    let bytes = m.encode_to_vec();
    let back = M::decode(bytes.as_slice()).unwrap();
    assert_eq!(&back, m);
}

#[test]
fn the_rejoin_messages_round_trip() {
    round_trip(&WorkspaceRejoinRequest {
        workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
        dry_run: true,
    });
    round_trip(&WorkspaceRejoinResponse {
        workspace: Some(WorkspaceInfo {
            workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
            root: "/home/a/todo".into(),
            ..WorkspaceInfo::default()
        }),
        backup_dir: "/home/a/todo.rejoin-backup-2026-09-26T150211Z".into(),
        moved: vec![
            ".txtodo".into(),
            "todo.txt".into(),
            "tasks/a/notes.md".into(),
        ],
        offering_devices: vec!["01ARZ3NDEKTSV4RRFFQ69G5FAV".into()],
    });
}
