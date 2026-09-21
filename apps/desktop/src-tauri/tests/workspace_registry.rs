//! `DaemonClient::workspace_add/remove/list/switch_workspace` (ADR 0025, task
//! `desktop-workspace-switcher`) against a real global `txtodod`: registers two workspaces,
//! switches the same connected client between them with no reconnect, and confirms
//! `list_files`/`workspace_list` actually reflect the switch and the removal — the Tauri commands
//! in `commands_workspace.rs` are thin wrappers over exactly this, so this is the real proof.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// Unix-only: see tests/daemon_spawn.rs's own doc comment (task desktop-windows-daemon-tests) for
// why -- same support::TXTODOD_BIN dependency, same fix.
//
// #[ignore]d (2026-09-19): CI-only, see tests/daemon_spawn.rs's own doc comment for the full
// rationale.
#![cfg(unix)]

mod support;

use desktop_lib::config::DesktopConfig;
use desktop_lib::daemon::DaemonClient;
use support::{TXTODOD_BIN, kill, temp_workspace, wait_for_global_pid};

/// A connected client against a fresh, hermetic global daemon, plus its pid and the `TempDir`
/// holding its socket/registry — both must outlive the client (mirrors `new_rpcs.rs`'s own
/// `connected_client` return shape); callers `kill(pid)` when done.
async fn connected() -> (DaemonClient, u32, tempfile::TempDir) {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let mut cfg = DesktopConfig::new(state_dir.path());
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    cfg.global_socket_override = Some(state_dir.path().join("txtodod.sock"));
    cfg.global_registry_override = Some(state_dir.path().join("registry.db"));
    let sock = desktop_lib::daemon::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon: {e}"));
    // No `wait_until_ready()` here: its Health probe carries `self.selector` (None, since no
    // workspace has been named yet) and a freshly spawned global daemon with zero workspaces
    // open correctly refuses an unselected Health call (FailedPrecondition) — the same "sole
    // open workspace" bridge every other caller relies on, not a readiness failure.
    // `ensure_daemon` above already confirmed the socket itself accepts connections;
    // `workspace_list` (registry-level, never selector-scoped) is this test's own readiness
    // probe instead.
    let mut client = DaemonClient::connect(&sock, None)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    client
        .workspace_list()
        .await
        .unwrap_or_else(|e| panic!("workspace_list (readiness probe): {e}"));
    let pid = wait_for_global_pid(state_dir.path());
    (client, pid, state_dir)
}

fn seeded_dir(seed: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), format!("{seed}\n"))
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    dir
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn add_list_remove_round_trip_and_add_is_idempotent() {
    let (mut client, pid, _state_dir) = connected().await;
    let dir = temp_workspace();

    // A fresh daemon always registers the reserved default workspace (task
    // `default-workspace`) — "empty" here means "no workspace but the default", not zero.
    assert!(
        client
            .workspace_list()
            .await
            .unwrap()
            .iter()
            .all(|w| w.is_default)
    );
    let first = client.workspace_add(dir.path()).await.unwrap();
    let again = client.workspace_add(dir.path()).await.unwrap();
    assert_eq!(first.workspace_id, again.workspace_id, "idempotent add");

    let listed = client.workspace_list().await.unwrap();
    let non_default: Vec<_> = listed.iter().filter(|w| !w.is_default).collect();
    assert_eq!(non_default.len(), 1);
    assert_eq!(non_default[0].workspace_id, first.workspace_id);

    assert!(client.workspace_remove(&first.workspace_id).await.unwrap());
    assert!(
        client
            .workspace_list()
            .await
            .unwrap()
            .iter()
            .all(|w| w.is_default)
    );
    // Registry::remove is an idempotent upsert-tombstone: an already-removed (but once-known) id
    // still returns true. false is reserved for an id this registry never heard of at all.
    assert!(
        client.workspace_remove(&first.workspace_id).await.unwrap(),
        "removing an already-removed id is idempotent, not an error"
    );
    assert!(
        !client
            .workspace_remove("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .await
            .unwrap(),
        "an id this registry never heard of returns false"
    );
    kill(pid);
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn switch_workspace_retargets_every_call_with_no_reconnect() {
    let (mut client, pid, _state_dir) = connected().await;
    let dir_a = seeded_dir("only in a");
    let dir_b = seeded_dir("only in b");
    client.workspace_add(dir_a.path()).await.unwrap();
    client.workspace_add(dir_b.path()).await.unwrap();

    client.switch_workspace(dir_a.path());
    let files_a = client.get_file("todo.txt").await.unwrap();
    assert_eq!(String::from_utf8_lossy(&files_a.bytes).trim(), "only in a");

    // Same client, same connection — just a different selector from here on.
    client.switch_workspace(dir_b.path());
    let files_b = client.get_file("todo.txt").await.unwrap();
    assert_eq!(String::from_utf8_lossy(&files_b.bytes).trim(), "only in b");
    kill(pid);
}

/// Task `desktop-no-assumed-workspace`: a client with no workspace selected must still come up
/// ready — the daemon refuses an unselected `Health` call while zero (or several) workspaces are
/// open, so readiness for an unbound client is probed through the registry instead. This is what
/// keeps a Finder-launched app (no workspace, nothing registered yet) from landing on "Daemon:
/// dead".
///
/// A fresh daemon still registers exactly one workspace on its own — the reserved default (task
/// `default-workspace`) — so "empty registry" here means "no workspace the user themselves
/// added", not zero rows.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn an_unbound_client_is_ready_and_lists_only_the_default_workspace() {
    let (mut client, pid, _state_dir) = connected().await;
    client
        .wait_until_ready()
        .await
        .unwrap_or_else(|e| panic!("an unbound client must be ready: {e}"));
    let listed = client.workspace_list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].is_default);
    kill(pid);
}
