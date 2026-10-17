//! The bind messages (task spike-txtodo-should-merge-remote-workspa) encode and decode to
//! themselves, both move lists and the nested `WorkspaceInfo` included.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use prost::Message;
use txtodo_proto::v1::{WorkspaceBindRequest, WorkspaceBindResponse, WorkspaceInfo};

fn round_trip<M: Message + Default + PartialEq + std::fmt::Debug>(m: &M) {
    let bytes = m.encode_to_vec();
    let back = M::decode(bytes.as_slice()).unwrap();
    assert_eq!(&back, m);
}

#[test]
fn the_bind_messages_round_trip() {
    round_trip(&WorkspaceBindRequest {
        workspace_id: "01M3ZKQNERX09CT41RBFJVE3AC".into(),
        root: "/home/a/txtodo".into(),
        dry_run: true,
    });
    round_trip(&WorkspaceBindResponse {
        workspace: Some(WorkspaceInfo {
            workspace_id: "01M3ZKQNERX09CT41RBFJVE3AC".into(),
            root: "/home/a/txtodo".into(),
            ..WorkspaceInfo::default()
        }),
        backup_dir: "/home/a/txtodo.rejoin-backup-2026-10-17T120000Z".into(),
        moved_aside: vec![".txtodo".into(), "todo.txt".into()],
        moved_in: vec![
            ".txtodo".into(),
            "todo.txt".into(),
            "tasks/a/notes.md".into(),
        ],
        replaced_workspace_id: "01M4MJ4YSYDGEBJWYB3F85B3DP".into(),
    });
}
