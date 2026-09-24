//! The per-area `Daemon` wrappers (task `tui-revamp/tui-foundation`) against a real `txtodod`:
//! each call reaches its RPC and comes back with what the screens will read. Unix-only and
//! `#[ignore]`d like the crate's other real-daemon tests (see `tests/daemon_autostart.rs`).
#![cfg(unix)]

mod support;

use txtodo_proto::v1 as pb;
use txtodo_tui::daemon::Daemon;

const TODO: &str = "buy milk\n(A) call mum due:2026-09-26\n";

/// Line `n`'s `TaskRef`, with the id the daemon holds for it (`GetFile`'s `task_ids`).
async fn line(daemon: &mut Daemon, n: u32) -> pb::TaskRef {
    let file = daemon
        .get_file("todo.txt")
        .await
        .unwrap_or_else(|e| panic!("get_file: {e}"));
    pb::TaskRef {
        line_number: n,
        task_id: file.task_ids[n as usize - 1].clone(),
    }
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn the_registry_and_universal_rows_answer() {
    let (_real, mut daemon) = support::RealDaemon::start(TODO).await;
    let listed = daemon
        .workspace_list()
        .await
        .unwrap_or_else(|e| panic!("workspace_list: {e}"));
    assert!(!listed.workspaces.is_empty(), "the bridge's own workspace");
    let rows = daemon
        .universal_tasks(false)
        .await
        .unwrap_or_else(|e| panic!("universal_tasks: {e}"));
    let mum = rows
        .tasks
        .iter()
        .find(|t| t.raw.contains("call mum"))
        .unwrap_or_else(|| panic!("no row: {:?}", rows.tasks));
    assert_eq!(
        (mum.priority.as_str(), mum.due.as_str()),
        ("A", "2026-09-26")
    );
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn notes_ref_dir_history_and_undo_answer() {
    let (_real, mut daemon) = support::RealDaemon::start(TODO).await;
    let first = line(&mut daemon, 1).await;
    daemon
        .edit_notes(first.clone(), "remember oat milk\n")
        .await
        .unwrap_or_else(|e| panic!("edit_notes: {e}"));
    let first = line(&mut daemon, 1).await;
    let notes = daemon
        .get_notes(first.clone())
        .await
        .unwrap_or_else(|e| panic!("get_notes: {e}"));
    assert_eq!(notes.bytes, b"remember oat milk\n");
    let dir = daemon
        .ref_dir("todo.txt", first, false)
        .await
        .unwrap_or_else(|e| panic!("ref_dir: {e}"));
    assert!(dir.has_ref_tag && dir.dir_exists, "{dir:?}");
    let history = daemon
        .history("todo.txt", 5)
        .await
        .unwrap_or_else(|e| panic!("history: {e}"));
    assert!(!history.ops.is_empty());
    daemon
        .undo("todo.txt", 1)
        .await
        .unwrap_or_else(|e| panic!("undo: {e}"));
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn tokens_and_devices_answer() {
    let (_real, mut daemon) = support::RealDaemon::start(TODO).await;
    let token = daemon
        .token_create("tui-test", vec!["read".to_owned()], "")
        .await
        .unwrap_or_else(|e| panic!("token_create: {e}"));
    assert!(!token.secret.is_empty(), "the secret comes back once");
    let tokens = daemon
        .token_list()
        .await
        .unwrap_or_else(|e| panic!("token_list: {e}"));
    assert!(tokens.tokens.iter().any(|t| t.id == token.id));
    daemon
        .token_revoke(&token.id)
        .await
        .unwrap_or_else(|e| panic!("token_revoke: {e}"));
    daemon
        .device_list()
        .await
        .unwrap_or_else(|e| panic!("device_list: {e}"));
}
