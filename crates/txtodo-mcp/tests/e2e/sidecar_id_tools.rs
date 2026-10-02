//! Real-daemon proof for task sidecar-task-ids: under Sidecar identity (the daemon's default) a line
//! has no `id:` tag, so every id-addressed tool used to answer "no task id:..." and `TaskRow.id`
//! was always empty. The ids now come from `GetFile`'s `task_ids`. Drives [`GrpcMcpBackend`]
//! directly, the backend `schema.rs`'s tool methods delegate to.
//!
//! In `tests/e2e` like `global_workspace_routing.rs` (see its module doc): it spawns a
//! separately built `txtodod` this crate may not depend on.

use crate::support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use txtodo_mcp::backend::{FieldPatch, GetTarget, ListArgs, McpBackend, MoveAnchor};
use txtodo_mcp::grpc_backend::GrpcMcpBackend;

/// `target/debug/txtodod`, built on first use by [`support::daemon_bin`] (this crate may not
/// depend on txtodo-daemon, so `CARGO_BIN_EXE_txtodod` does not exist here).
fn daemon_binary() -> PathBuf {
    support::daemon_bin().to_path_buf()
}

/// Kills the spawned `txtodod` even if an assertion panics later.
struct KillOnDrop(std::process::Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Waits (up to 60 s: a fresh binary's first exec is slow on a loaded macOS) for the socket.
async fn wait_for_socket(path: &Path) {
    for _ in 0..600 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("socket {path:?} never appeared");
}

/// One empty workspace under a real `txtodod` in global mode, Sidecar identity, isolated to `tmp`.
#[allow(clippy::expect_used)]
async fn start_sidecar_daemon(tmp: &Path) -> (KillOnDrop, PathBuf, String) {
    let socket = tmp.join("txtodod.sock");
    let ws = tmp.join("ws");
    std::fs::create_dir_all(&ws).expect("mkdir ws");
    std::fs::write(ws.join("todo.txt"), "").expect("seed todo.txt");
    let daemon = std::process::Command::new(daemon_binary())
        .arg("--identity-mode")
        .arg("sidecar")
        .env("TXTODO_SOCKET", &socket)
        .env("TXTODO_REGISTRY_DB", tmp.join("registry.db"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn txtodod");
    wait_for_socket(&socket).await;
    (
        KillOnDrop(daemon),
        socket,
        ws.to_string_lossy().into_owned(),
    )
}

#[tokio::test]
async fn id_addressed_tools_resolve_a_sidecar_task() {
    // The OS temp dir keeps the unix socket path under SUN_LEN (~104 bytes).
    let tmp = tempfile::tempdir().expect("tempdir");
    let (_guard, socket, ws) = start_sidecar_daemon(tmp.path()).await;
    let backend = GrpcMcpBackend::connect_unix(&socket, None)
        .await
        .expect("connect to the global socket");
    let w = || Some(ws.clone());

    // `todo_add` hands back the id although the line carries no tag.
    let one = backend.add("one".into(), None, w()).await.expect("add");
    let two = backend.add("two".into(), None, w()).await.expect("add");
    assert!(!one.raw.contains("id:"), "sidecar text: {}", one.raw);
    let (one_id, two_id) = (one.id.expect("row id"), two.id.expect("row id"));
    assert_eq!(one_id.len(), 26, "a ULID");
    assert_ne!(one_id, two_id);

    // `todo_list` rows carry ids too, and `todo_get` finds a task by one.
    let rows = backend
        .list(ListArgs {
            workspace: w(),
            ..ListArgs::default()
        })
        .await
        .expect("list");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].id.as_deref(), Some(two_id.as_str()));
    let got = backend
        .get(GetTarget {
            id: Some(two_id.clone()),
            workspace: w(),
            ..GetTarget::default()
        })
        .await
        .expect("get by id");
    assert_eq!(got.line, 2);

    // Edit, move and complete by id; the id stays with the task through all three.
    let patch = FieldPatch {
        append: Some("+proj".into()),
        ..FieldPatch::default()
    };
    let edited = backend
        .edit(two_id.clone(), patch, w())
        .await
        .expect("edit");
    assert!(edited.raw.ends_with("+proj"), "{}", edited.raw);
    let moved = backend
        .move_task(two_id.clone(), MoveAnchor::Before(one_id.clone()), w())
        .await
        .expect("move");
    assert_eq!(moved.line, 1);
    let done = backend
        .complete(one_id.clone(), true, w())
        .await
        .expect("complete");
    assert!(done.done);
    assert_eq!(done.id.as_deref(), Some(one_id.as_str()));

    // Notes resolve by task id alone on the daemon side.
    backend
        .notes_set(two_id.clone(), "# two\n".into(), w())
        .await
        .expect("notes_set");
    let notes = backend.notes_get(two_id, w()).await.expect("notes_get");
    assert_eq!(notes, "# two\n");
}

/// Task workspace-layout: with `todo_file = "work.txt"` in `txtodo.toml`, a tool that names no
/// file adds to, lists and reads `work.txt`, not a `todo.txt` that does not exist.
#[tokio::test]
async fn tools_with_no_file_use_the_layouts_root_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let socket = tmp.path().join("txtodod.sock");
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).expect("mkdir ws");
    std::fs::write(ws.join("txtodo.toml"), "todo_file = \"work.txt\"\n").expect("seed toml");
    std::fs::write(ws.join("work.txt"), "").expect("seed work.txt");
    let daemon = std::process::Command::new(daemon_binary())
        .env("TXTODO_SOCKET", &socket)
        .env("TXTODO_REGISTRY_DB", tmp.path().join("registry.db"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn txtodod");
    let _guard = KillOnDrop(daemon);
    wait_for_socket(&socket).await;
    let backend = GrpcMcpBackend::connect_unix(&socket, None)
        .await
        .expect("connect to the global socket");
    let w = || Some(ws.to_string_lossy().into_owned());

    backend
        .add("plan the launch".into(), None, w())
        .await
        .expect("add");
    let rows = backend
        .list(ListArgs {
            workspace: w(),
            ..ListArgs::default()
        })
        .await
        .expect("list");
    assert_eq!(rows.len(), 1, "{rows:?}");
    let text = backend.get_file(None, w()).await.expect("get_file");
    assert!(text.contains("plan the launch"), "{text}");
    let on_disk = std::fs::read_to_string(ws.join("work.txt")).expect("read work.txt");
    assert!(on_disk.contains("plan the launch"), "{on_disk}");
    assert!(!ws.join("todo.txt").exists(), "no todo.txt was invented");
}
