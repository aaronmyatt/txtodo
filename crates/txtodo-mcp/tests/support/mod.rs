//! Shared harness for this crate's real-`txtodod` integration tests (`tests/daemon_autostart.rs`;
//! `tests/global_workspace_routing.rs` keeps its own inline `daemon_binary()`/`KillOnDrop` for now
//! rather than being migrated here, out of scope for this change). Not a test binary itself: a
//! `mod.rs` under a `tests/` subdirectory is cargo's documented way to share code between
//! integration-test binaries without it being collected as its own test target.
//! Ref: <https://doc.rust-lang.org/book/ch11-03-test-organization.html#submodules-in-integration-tests>
#![allow(dead_code)] // not every test file uses every helper here

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// How long to wait for a socket/pid file to appear. Generous: a debug build on a loaded CI
/// runner is slow (same rationale as `txtodo-tui`'s and `apps/desktop`'s own harnesses).
pub const WAIT: Duration = Duration::from_secs(60);

/// `crates/txtodo-mcp` -> `crates` -> the workspace root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("CARGO_MANIFEST_DIR has no grandparent"))
        .to_path_buf()
}

/// Finds `txtodod`'s executable, building it once via `cargo build -p txtodo-daemon --bin
/// txtodod` if `target/debug/txtodod` doesn't exist yet. No Cargo dependency edge onto
/// `txtodo-daemon` is added by this (`budgets.json`'s `allowedDeps` forbids it) — this locates the
/// binary by its ordinary `target/` path instead, the same approach
/// `crates/txtodo-tui/tests/support/mod.rs::daemon_bin` and
/// `apps/desktop/src-tauri/tests/support/mod.rs::TXTODOD_BIN` already use. `OnceLock` so every
/// test in one binary run pays the build cost once.
pub fn daemon_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = workspace_root();
        let bin = root.join("target").join("debug").join("txtodod");
        if !bin.is_file() {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "txtodo-daemon", "--bin", "txtodod"])
                .current_dir(&root)
                .status()
                .unwrap_or_else(|e| panic!("cargo build txtodod: {e}"));
            assert!(status.success(), "cargo build -p txtodo-daemon failed");
        }
        assert!(bin.is_file(), "expected {} to exist", bin.display());
        bin
    })
}

/// A temp workspace with one todo.txt, ready for a daemon to adopt.
pub fn temp_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(dir.path().join("todo.txt"), "buy milk\n")
        .unwrap_or_else(|e| panic!("write todo.txt: {e}"));
    dir
}

/// Waits for `path` to hold a parseable pid and returns it — the `--dir <workspace>` bridge
/// daemon's single-instance lock (`<workspace>/.txtodo/txtodod.pid`,
/// `crates/txtodo-daemon/src/pidfile.rs`'s own doc comment).
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

/// Best-effort SIGKILL by pid, for test cleanup — matches
/// `apps/desktop/src-tauri/tests/support/mod.rs::kill`'s own precedent (SIGKILL, not `-TERM`:
/// graceful shutdown is the daemon's own concern, not something a test needs to wait on).
pub fn kill(pid: u32) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
}
