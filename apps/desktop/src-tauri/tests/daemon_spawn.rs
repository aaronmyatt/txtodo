//! Acceptance test for `tasks/desktop-tauri-shell`: a fresh install with no daemon running gets
//! one spawned and can `list_files` within the spawn timeout, and a daemon already running (as
//! if started by the CLI) is reused rather than duplicated — exactly one `txtodod` process per
//! workspace. Spawn/build helpers live in `tests/support/mod.rs`, shared with `tests/new_rpcs.rs`.

mod support;

use desktop_lib::config::DesktopConfig;
use desktop_lib::daemon::{self, DaemonClient};
use std::process::{Command, Stdio};
use std::time::Instant;
use support::{TXTODOD_BIN, kill, temp_workspace, wait_for_pid};

#[tokio::test]
async fn fresh_install_spawns_daemon_and_lists_files() {
    let bin = TXTODOD_BIN.clone();
    let dir = temp_workspace();
    let mut cfg = DesktopConfig::new(dir.path());
    cfg.daemon_bin = Some(bin);

    let start = Instant::now();
    let sock = daemon::ensure_daemon(&cfg)
        .await
        .expect("ensure_daemon should spawn a fresh txtodod");
    let mut client = DaemonClient::connect(&sock)
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
        "todo.txt should be adopted: {files:?}"
    );

    kill(wait_for_pid(dir.path()));
}

#[tokio::test]
async fn already_running_daemon_is_reused_not_duplicated() {
    let bin = TXTODOD_BIN.clone();
    let dir = temp_workspace();

    // Simulate "started by the CLI": spawn txtodod directly, outside of ensure_daemon.
    let mut manual = Command::new(&bin)
        .arg("--dir")
        .arg(dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn txtodod directly");
    let pid_before = wait_for_pid(dir.path());
    assert_eq!(
        pid_before,
        manual.id(),
        "pid file names the daemon we just spawned"
    );

    // ensure_daemon on the same workspace must reuse it, never spawn a second one.
    let mut cfg = DesktopConfig::new(dir.path());
    cfg.daemon_bin = Some(bin);
    let sock = daemon::ensure_daemon(&cfg)
        .await
        .expect("ensure_daemon should reuse the already-running daemon");
    let mut client = DaemonClient::connect(&sock).await.expect("connect");
    client
        .wait_until_ready()
        .await
        .expect("the already-running daemon should already be ready");
    client
        .list_files()
        .await
        .expect("list_files should succeed against the reused daemon");

    let pid_after = wait_for_pid(dir.path());
    assert_eq!(
        pid_before, pid_after,
        "exactly one txtodod process per workspace"
    );

    kill(pid_after);
    let _ = manual.wait();
}
