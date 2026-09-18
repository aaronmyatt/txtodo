//! Shared harness for the real-`txtodod` integration tests (recommended build order step 5):
//! spawns the daemon binary on a temp workspace, wraps a [`Daemon`] against its socket, and
//! writes files "from outside" to simulate an external edit. Slice-local by design, mirroring
//! `crates/txtodo-daemon/tests/support/mod.rs`'s own doc ("constitution §7: no cross-slice
//! helpers") — this crate cannot depend on `txtodo-daemon` (`allowedDeps["txtodo-tui"]` is
//! `[txtodo-core, txtodo-proto]` only), so `CARGO_BIN_EXE_txtodod` is never set here (Cargo only
//! sets that variable for a binary in the *same* package as the test). [`daemon_bin`] locates —
//! and, the first time, builds — the binary by its ordinary `target/` path instead.
#![allow(dead_code)] // each test file uses a different subset of these helpers

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use txtodo_tui::daemon::{Daemon, socket_path};

/// How long to wait for the socket after spawn, and for `Daemon::wait_until_ready`. Generous, the
/// same rationale as `txtodo-daemon`'s own harness: a debug build on a loaded CI runner is slow.
pub const SOCKET_WAIT: Duration = Duration::from_secs(60);

/// Finds `txtodod`'s executable, building it once via `cargo build -p txtodo-daemon --bin
/// txtodod` if `target/debug/txtodod` doesn't exist yet (e.g. a bare `cargo test -p txtodo-tui`
/// that never built the daemon crate). Cached so every test in one run pays the build cost once.
pub fn daemon_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let target_dir = workspace_root().join("target");
        let bin = target_dir.join("debug").join("txtodod");
        if !bin.is_file() {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "txtodo-daemon", "--bin", "txtodod"])
                .current_dir(workspace_root())
                .status()
                .unwrap_or_else(|e| panic!("cargo build txtodod: {e}"));
            assert!(status.success(), "cargo build txtodod failed");
        }
        assert!(bin.is_file(), "{} not built", bin.display());
        bin
    })
}

/// `crates/txtodo-tui` -> `crates` -> the workspace root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("CARGO_MANIFEST_DIR has no grandparent"))
        .to_path_buf()
}

/// A running `txtodod` on its own temp workspace, plus a [`Daemon`] dialed against its socket.
pub struct RealDaemon {
    pub dir: tempfile::TempDir,
    child: Child,
}

impl RealDaemon {
    /// Writes `todo` as `todo.txt`, spawns `txtodod --dir <tmp>`, waits for the socket, and
    /// returns once a real [`Daemon`] client is ready to talk to it.
    pub async fn start(todo: &str) -> (RealDaemon, Daemon) {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        std::fs::write(dir.path().join("todo.txt"), todo).unwrap_or_else(|e| panic!("{e}"));
        let child = Command::new(daemon_bin())
            .args(["--dir", &dir.path().to_string_lossy()])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
        let sock = socket_path(dir.path());
        let start = Instant::now();
        while !sock.exists() {
            assert!(start.elapsed() < SOCKET_WAIT, "socket did not appear");
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut daemon = Daemon::connect(&sock, None)
            .await
            .unwrap_or_else(|e| panic!("connect: {e}"));
        daemon
            .wait_until_ready()
            .await
            .unwrap_or_else(|e| panic!("daemon never became ready: {e}"));
        (RealDaemon { dir, child }, daemon)
    }

    /// Bytes on disk right now (an "external" editor's view).
    pub fn disk(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("todo.txt")).unwrap_or_default()
    }

    /// An editor-style save: overwrite the whole file, as if from outside the TUI.
    pub fn external_write(&self, text: &str) {
        std::fs::write(self.dir.path().join("todo.txt"), text).unwrap_or_else(|e| panic!("{e}"));
    }
}

impl Drop for RealDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Polls `f` until it returns `Some`, or panics after `timeout`.
pub async fn wait_for<T>(timeout: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < timeout, "timed out waiting for condition");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
