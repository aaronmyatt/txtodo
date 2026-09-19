//! `DaemonClient::get_file_for` (ADR 0025, task `desktop-universal-view`) against a real global
//! `txtodod`: fetches another registered workspace's `todo.txt` by an explicit `WorkspaceId`
//! selector without disturbing the client's own `selector` field — `commands_universal::
//! universal_tasks` (untestable directly here; it takes a Tauri `AppHandle`/`State`, see
//! `tests/new_rpcs.rs`'s module doc for why these tests exercise `DaemonClient` directly instead)
//! is a thin loop over exactly this plus `open_tasks`, which has its own unit test alongside the
//! parsing logic in `src/commands_universal.rs`.
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
use txtodo_proto::v1 as pb;

/// `temp_workspace()`'s seeded `todo.txt` ("buy milk"), overwritten with `seed` — reuses the
/// shared helper (so `support`'s dead-code lint stays satisfied) rather than duplicating its
/// tempdir/write logic for one different line of content.
fn seeded_dir(seed: &str) -> tempfile::TempDir {
    let dir = temp_workspace();
    std::fs::write(dir.path().join("todo.txt"), format!("{seed}\n"))
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    dir
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn get_file_for_reaches_another_workspace_without_moving_the_clients_own_selector() {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let mut cfg = DesktopConfig::new(state_dir.path());
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    cfg.global_socket_override = Some(state_dir.path().join("txtodod.sock"));
    cfg.global_registry_override = Some(state_dir.path().join("registry.db"));
    let sock = desktop_lib::daemon::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("ensure_daemon: {e}"));

    let dir_a = seeded_dir("(A) 2026-09-15 only in a @home");
    let dir_b = seeded_dir("(B) 2026-09-15 only in b @work");

    let selector_a = pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::Path(
            dir_a.path().display().to_string(),
        )),
    };
    let mut client = DaemonClient::connect(&sock, Some(selector_a))
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    let pid = wait_for_global_pid(state_dir.path());

    let workspace_a = client
        .workspace_add(dir_a.path())
        .await
        .expect("register a");
    let workspace_b = client
        .workspace_add(dir_b.path())
        .await
        .expect("register b");

    let selector_b = pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::WorkspaceId(
            workspace_b.workspace_id.clone(),
        )),
    };
    let from_b = client
        .get_file_for(selector_b, "todo.txt")
        .await
        .expect("get_file_for b");
    assert_eq!(
        String::from_utf8_lossy(&from_b.bytes).trim(),
        "(B) 2026-09-15 only in b @work"
    );

    // The client's own selector (set at connect() time, targeting dir_a) must be untouched by the
    // call above — plain get_file() still resolves against workspace a, not whatever
    // get_file_for() last reached.
    let still_a = client.get_file("todo.txt").await.expect("get_file a");
    assert_eq!(
        String::from_utf8_lossy(&still_a.bytes).trim(),
        "(A) 2026-09-15 only in a @home"
    );
    assert_ne!(workspace_a.workspace_id, workspace_b.workspace_id);

    kill(pid);
}
