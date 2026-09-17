//! Real-`txtodod` acceptance tests for [`txtodo_daemon_launch::ensure_daemon`] (task
//! `daemon-always-available`, item 7: "per-client cold-start... connects without manual
//! intervention"). Mirrors `apps/desktop/src-tauri/tests/daemon_spawn.rs`'s shape (hermetic
//! socket via env override, build the real binary once via `tests/support`), generalized over
//! both target shapes this crate supports: the ADR 0025 global daemon (no extra argv) and a
//! legacy per-workspace bridge daemon (`--dir <workspace>`, what `txtodo-tui` dials today).
#![cfg(unix)]

mod support;

use std::time::Instant;
use support::{TXTODOD_BIN, WAIT, kill, wait_for_pid};
use txtodo_daemon_launch::{LaunchConfig, ensure_daemon};

#[tokio::test]
async fn cold_start_spawns_the_global_daemon() {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let socket = state_dir.path().join("txtodod.sock");
    let registry = state_dir.path().join("registry.db");
    let mut cfg = LaunchConfig::new(&socket);
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    cfg.extra_env = vec![
        ("TXTODO_SOCKET".to_owned(), socket.display().to_string()),
        (
            "TXTODO_REGISTRY_DB".to_owned(),
            registry.display().to_string(),
        ),
    ];

    let start = Instant::now();
    ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon should spawn a fresh global txtodod: {e}"));
    assert!(
        socket.exists(),
        "the socket exists once ensure_daemon returns Ok"
    );
    assert!(start.elapsed() < WAIT, "stayed within the wait budget");

    kill(wait_for_pid(&state_dir.path().join("txtodod.pid")));
}

#[tokio::test]
async fn cold_start_is_idempotent_against_an_already_running_daemon() {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let socket = state_dir.path().join("txtodod.sock");
    let registry = state_dir.path().join("registry.db");
    let mut cfg = LaunchConfig::new(&socket);
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    cfg.extra_env = vec![
        ("TXTODO_SOCKET".to_owned(), socket.display().to_string()),
        (
            "TXTODO_REGISTRY_DB".to_owned(),
            registry.display().to_string(),
        ),
    ];

    ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("first ensure_daemon: {e}"));
    let pid_before = wait_for_pid(&state_dir.path().join("txtodod.pid"));

    // A second call against the same, still-live socket must reuse it, never spawn a second one.
    ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("second ensure_daemon should reuse the live daemon: {e}"));
    let pid_after = wait_for_pid(&state_dir.path().join("txtodod.pid"));
    assert_eq!(
        pid_before, pid_after,
        "exactly one txtodod, not a second spawn"
    );

    kill(pid_after);
}

#[tokio::test]
async fn legacy_bridge_daemon_spawns_at_the_workspace_socket() {
    let workspace = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(workspace.path().join("todo.txt"), "buy milk\n")
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    let socket = workspace.path().join(".txtodo").join("txtodod.sock");
    let mut cfg = LaunchConfig::new(&socket).with_dir(workspace.path());
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());

    ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon should spawn the --dir bridge daemon: {e}"));
    assert!(socket.exists());

    kill(wait_for_pid(
        &workspace.path().join(".txtodo").join("txtodod.pid"),
    ));
}
