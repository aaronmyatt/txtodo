//! Shared spawn helpers for this crate's daemon-backed integration tests (`tests/daemon_spawn.rs`,
//! `tests/new_rpcs.rs`). Not a test binary itself: a `mod.rs` under a `tests/` subdirectory is
//! cargo's documented way to share code between integration-test binaries without it being
//! collected as its own test target.
//! Ref: <https://doc.rust-lang.org/book/ch11-03-test-organization.html#submodules-in-integration-tests>

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// How long to wait for any one filesystem/process condition in these tests.
pub const WAIT: Duration = Duration::from_secs(60);

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
/// tests that run concurrently share one build instead of racing redundant `cargo build`
/// subprocesses against the same lock.
pub static TXTODOD_BIN: LazyLock<PathBuf> = LazyLock::new(|| {
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
pub fn temp_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "buy milk\n")
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    dir
}

/// Waits for the global daemon's pid file (`<state_dir>/txtodod.pid`, beside a hermetic test's
/// own `global_socket_override`) to hold a number and returns it — the ADR 0025 analogue of
/// `wait_for_pid` above, which is the pre-M11 per-workspace location.
pub fn wait_for_global_pid(state_dir: &Path) -> u32 {
    wait_for_pid_at(&state_dir.join("txtodod.pid"))
}

fn wait_for_pid_at(path: &Path) -> u32 {
    let start = Instant::now();
    loop {
        if let Some(pid) = std::fs::read_to_string(path)
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
/// still gets cleaned up this way. SIGKILL (not `-TERM`), matching `crates/txtodo-daemon/tests/support`'s
/// `Drop` impl: graceful shutdown is the daemon's own concern, not something a test needs to wait on.
pub fn kill(pid: u32) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
}
