//! The named-list messages (task two-kinds-of-workspace, ADR 0043) encode and decode to
//! themselves, and a `WorkspaceInfo` from an older daemon reads as kind unknown.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use prost::Message;
use txtodo_proto::v1::{
    ListEnsureRequest, ListEnsureResponse, ListRemoveRequest, ListRemoveResponse,
    ListRenameRequest, WorkspaceInfo, WorkspaceKind,
};

fn round_trip<M: Message + Default + PartialEq + std::fmt::Debug>(m: &M) {
    let bytes = m.encode_to_vec();
    let back = M::decode(bytes.as_slice()).unwrap();
    assert_eq!(&back, m);
}

#[test]
fn the_list_messages_round_trip() {
    round_trip(&ListEnsureRequest {
        name: "work".into(),
    });
    round_trip(&ListEnsureResponse {
        workspace: Some(WorkspaceInfo {
            workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
            root: "/home/a/.local/share/txtodo/lists/work".into(),
            kind: WorkspaceKind::List as i32,
            ..WorkspaceInfo::default()
        }),
        created: true,
    });
    round_trip(&ListRenameRequest {
        list: "work".into(),
        new_name: "projects".into(),
    });
    round_trip(&ListRemoveRequest {
        list: "projects".into(),
    });
    round_trip(&ListRemoveResponse {
        removed: true,
        moved_to: "/home/a/.local/share/txtodo/lists/projects.removed-20261025".into(),
    });
}

#[test]
fn a_workspace_info_without_the_kind_field_reads_as_kind_unknown() {
    // What an older daemon sends: every field it knows, and no field 13.
    let older = WorkspaceInfo {
        workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
        root: "/home/a/todo".into(),
        ..WorkspaceInfo::default()
    };
    let back = WorkspaceInfo::decode(older.encode_to_vec().as_slice()).unwrap();
    assert_eq!(back.kind(), WorkspaceKind::Unspecified, "{back:?}");
}

#[test]
fn the_kind_names_a_list_and_a_repo() {
    for kind in [WorkspaceKind::List, WorkspaceKind::Repo] {
        let info = WorkspaceInfo {
            kind: kind as i32,
            ..WorkspaceInfo::default()
        };
        let back = WorkspaceInfo::decode(info.encode_to_vec().as_slice()).unwrap();
        assert_eq!(back.kind(), kind);
    }
}
