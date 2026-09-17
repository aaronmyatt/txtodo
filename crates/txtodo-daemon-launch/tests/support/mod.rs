//! Shared spawn helper for this crate's real-`txtodod` integration tests, mirroring
//! `apps/desktop/src-tauri/tests/support/mod.rs` exactly (same "build the daemon once, share it
//! across the whole test binary run" shape) since this crate has no `txtodo-daemon` dependency
//! (`.claude/budgets.json`'s `allowedDeps`) and cannot get `CARGO_BIN_EXE_txtodod` set for it —
//! that variable is only set by cargo for a binary in its *own* package.
//! Ref: <https://doc.rust-lang.org/book/ch11-03-test-organization.html#submodules-in-integration-tests>
#![allow(dead_code)] // each test file uses a different subset of these helpers

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// How long to wait for any one filesystem/process condition in these tests.
pub const WAIT: Duration = Duration::from_secs(60);

/// `crates/txtodo-daemon-launch` -> `crates` -> the workspace root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("CARGO_MANIFEST_DIR has no grandparent"))
        .to_path_buf()
}

/// Builds `txtodod` (debug) once per test binary run and returns its path.
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

/// Waits for a pid file to hold a number and returns it.
pub fn wait_for_pid(path: &Path) -> u32 {
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

/// Best-effort SIGKILL by pid, for test cleanup.
pub fn kill(pid: u32) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
}
