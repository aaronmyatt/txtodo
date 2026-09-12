//! Acceptance test for `tasks/desktop-tauri-shell`: a fresh install with no daemon running gets
//! one spawned and can `list_files` within the spawn timeout, and a daemon already running (as
//! if started by the CLI) is reused rather than duplicated — exactly one `txtodod` process per
//! workspace. Spirit of `crates/txtodo-daemon/tests/support`, but this crate has no dependency
//! edge on `txtodo-daemon` (only `txtodo-proto`, per the task's boundary), so it builds and
//! locates the real `txtodod` binary itself instead of relying on `CARGO_BIN_EXE_txtodod`.

use desktop_lib::config::DesktopConfig;
use desktop_lib::daemon::{self, DaemonClient};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// How long to wait for any one filesystem/process condition in this test file.
const WAIT: Duration = Duration::from_secs(60);

/// `apps/desktop/src-tauri` -> `apps/desktop` -> `apps` -> the workspace root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("three ancestors above src-tauri is the repo root"))
        .to_path_buf()
}

/// Builds `txtodod` (debug) once per test binary run and returns its path, without adding a
/// Cargo dependency edge from this crate onto `txtodo-daemon`. `LazyLock` (not a plain fn) so
/// the two tests in this file — which `cargo test` runs concurrently by default — share one
/// build instead of racing two redundant `cargo build` subprocesses against the same lock.
static TXTODOD_BIN: LazyLock<PathBuf> = LazyLock::new(|| {
    let root = workspace_root();
    let status = Command::new("cargo")
        .args(["build", "-p", "txtodo-daemon", "--bin", "txtodod"])
        .current_dir(&root)
        .status()
        .unwrap_or_else(|e| panic!("spawn cargo build: {e}"));
    assert!(status.success(), "cargo build -p txtodo-daemon failed");
    let bin = root.join("target").join("debug").join("txtodod");
    assert!(bin.is_file(), "expected {} to exist", bin.display());
    bin
});

/// A temp workspace with one todo.txt, ready for a daemon to adopt.
fn temp_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "buy milk\n")
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    dir
}

/// Waits for `<workspace>/.txtodo/txtodod.pid` to hold a number and returns it.
fn wait_for_pid(workspace: &Path) -> u32 {
    let path = workspace.join(".txtodo").join("txtodod.pid");
    let start = Instant::now();
    loop {
        if let Some(pid) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
        {
            return pid;
        }
        assert!(
            start.elapsed() < WAIT,
            "pid file never appeared at {path:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Best-effort SIGKILL by pid, for test cleanup; a daemon we never spawned (already reused)
/// still gets cleaned up this way. SIGKILL (not `-TERM`), matching the `Drop` impl in
/// `crates/txtodo-daemon/tests/support` (`Child::kill`): graceful shutdown is the daemon's own
/// concern, not something this test needs to wait on.
fn kill(pid: u32) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
}

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
