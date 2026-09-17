//! Acceptance test for `desktop-workspace-switcher` (ADR 0025, M11): a fresh install with no
//! global daemon running gets one spawned (no `--dir`) and can `list_files` against a registered
//! workspace within the spawn timeout, and a global daemon already running (as if started by the
//! CLI, or another desktop window) is reused rather than duplicated — exactly one `txtodod`
//! process for the whole device, not one per workspace. `global_socket_override`/
//! `global_registry_override` give each test its own hermetic global socket/registry without
//! mutating this process' shared environment (see `config.rs`'s own doc on why). Spawn/build
//! helpers live in `tests/support/mod.rs`, shared with `tests/new_rpcs.rs`.
//!
//! Unix-only (task `desktop-windows-daemon-tests`): `support::TXTODOD_BIN` unconditionally
//! `cargo build -p txtodo-daemon`, which does not compile on Windows at all (ADR 0010, already
//! excluded from `ci.yml`'s own typecheck/lint/test steps there) — this crate itself has no such
//! exclusion, so this file's tests reached that build and failed on `windows-latest` for every PR
//! touching `apps/desktop` or `txtodo-daemon` (found on PR #4, 2026-09-16). Same `#![cfg(unix)]`
//! pattern `crates/txtodo-daemon`'s own real-daemon test files already use (e.g.
//! `tests/lan_discovery.rs`).
#![cfg(unix)]

mod support;

use desktop_lib::config::DesktopConfig;
use desktop_lib::daemon::{self, DaemonClient};
use std::process::{Command, Stdio};
use std::time::Instant;
use support::{TXTODOD_BIN, kill, temp_workspace, wait_for_global_pid};
use txtodo_proto::v1 as pb;

fn hermetic_config(state_dir: &std::path::Path) -> DesktopConfig {
    let mut cfg = DesktopConfig::new(state_dir); // unused as a workspace by these tests
    cfg.daemon_bin = Some(TXTODOD_BIN.clone());
    cfg.global_socket_override = Some(state_dir.join("txtodod.sock"));
    cfg.global_registry_override = Some(state_dir.join("registry.db"));
    cfg
}

fn path_selector(path: &std::path::Path) -> pb::WorkspaceSelector {
    pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::Path(
            path.display().to_string(),
        )),
    }
}

#[tokio::test]
async fn fresh_install_spawns_the_global_daemon_and_lists_files() {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let cfg = hermetic_config(state_dir.path());
    let ws = temp_workspace();

    let start = Instant::now();
    let sock = daemon::ensure_daemon(&cfg)
        .await
        .expect("ensure_daemon should spawn a fresh global txtodod");
    let mut client = DaemonClient::connect(&sock, Some(path_selector(ws.path())))
        .await
        .expect("connect should build a lazy channel");
    client
        .wait_until_ready()
        .await
        .expect("the freshly spawned daemon should become ready");
    let files = client
        .list_files()
        .await
        .expect("list_files should succeed once the daemon is ready");
    assert!(
        start.elapsed() < cfg.spawn_timeout,
        "stayed within the spawn timeout budget"
    );
    assert!(
        files.files.iter().any(|f| f.path == "todo.txt"),
        "todo.txt should be adopted via the Path selector's auto-register: {files:?}"
    );

    kill(wait_for_global_pid(state_dir.path()));
}

#[tokio::test]
async fn already_running_global_daemon_is_reused_not_duplicated() {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let cfg = hermetic_config(state_dir.path());

    // Simulate "started by the CLI" (or another desktop window): spawn the global daemon
    // directly, outside of ensure_daemon.
    let mut manual = Command::new(&*TXTODOD_BIN)
        .env(
            "TXTODO_SOCKET",
            cfg.global_socket_override.as_ref().unwrap(),
        )
        .env(
            "TXTODO_REGISTRY_DB",
            cfg.global_registry_override.as_ref().unwrap(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn txtodod directly");
    let pid_before = wait_for_global_pid(state_dir.path());
    assert_eq!(
        pid_before,
        manual.id(),
        "pid file names the daemon we just spawned"
    );

    // ensure_daemon must reuse it, never spawn a second one.
    let sock = daemon::ensure_daemon(&cfg)
        .await
        .expect("ensure_daemon should reuse the already-running global daemon");
    // No workspace is open yet on this freshly spawned daemon, so an unselected Health call
    // would correctly fail FailedPrecondition ("no workspace is open") — the same "sole open
    // workspace" bridge every other global-daemon caller relies on; name one via a Path selector.
    let ws = temp_workspace();
    let mut client = DaemonClient::connect(&sock, Some(path_selector(ws.path())))
        .await
        .expect("connect");
    client
        .wait_until_ready()
        .await
        .expect("the already-running daemon should already be ready");

    let pid_after = wait_for_global_pid(state_dir.path());
    assert_eq!(
        pid_before, pid_after,
        "exactly one txtodod process for the whole device"
    );

    kill(pid_after);
    let _ = manual.wait();
}
