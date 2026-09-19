//! `do` + auto-archive against a real tagged-mode `txtodod`: the reorder must reach the daemon as
//! guarded `MoveToEnd` mutations (`archive_plan.rs`), not as a whole-file write the daemon then
//! reconciles as an External edit (which can drop another writer's concurrent `Apply`). Every
//! change here goes through the socket, so `log` must show no `external@` op. Tagged mode because
//! the plan needs `id:` tags in the text; the daemon defaults to sidecar, which has none.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// How long to wait for the daemon socket.
const SOCKET_WAIT: Duration = Duration::from_secs(20);

/// `target/debug/deps/<test>` → `target/debug/txtodod`, built on demand: this crate may not depend
/// on txtodo-daemon (slice rule), so the socket is the boundary. Same as `daemon_mode.rs`.
fn txtodod_binary() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap();
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    let bin = dir.join(format!("txtodod{}", std::env::consts::EXE_SUFFIX));
    if !bin.exists() {
        let built = Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "txtodo-daemon",
                "--bin",
                "txtodod",
                "--quiet",
            ])
            .status()
            .unwrap();
        assert!(built.success(), "building txtodod failed");
    }
    bin
}

struct Daemon(Child);

impl Daemon {
    fn spawn_tagged(dir: &Path) -> Daemon {
        let child = Command::new(txtodod_binary())
            .args(["--dir", &dir.to_string_lossy(), "--identity-mode", "tagged"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let socket = dir.join(".txtodo").join("txtodod.sock");
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < SOCKET_WAIT,
                "daemon socket did not appear"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Daemon(child)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Same isolation as `daemon_mode.rs::txtodo`: no ambient global daemon or real config can leak in.
fn txtodo(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .env("XDG_DATA_HOME", dir.join(".global-home"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Each line as `a` (open) or `x:a` (done): drops the dates and the `id:` tag.
fn tasks(dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join("todo.txt")).unwrap();
    let name = |l: &str| {
        l.split(" id:")
            .next()
            .unwrap()
            .rsplit(' ')
            .next()
            .unwrap()
            .to_owned()
    };
    text.lines()
        .map(|l| {
            if l.starts_with("x ") {
                format!("x:{}", name(l))
            } else {
                name(l)
            }
        })
        .collect()
}

// CI-only: spawns a real txtodod (see `daemon_mode.rs`); run via `cargo test -- --ignored`.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[test]
fn do_archives_through_move_to_end_never_a_whole_file_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn_tagged(dir.path());
    // Added through the daemon, so its history holds no External op (an adopted seed file would).
    for name in ["a", "b", "c", "d"] {
        txtodo(dir.path(), &["add", name]);
    }
    txtodo(dir.path(), &["do", "1"]);
    assert_eq!(tasks(dir.path()), ["b", "c", "d", "x:a"]);
    // A done line already sits at the bottom, and `b` was above it: archive keeps that order, so
    // the plan has to move `a` again, behind `b`.
    txtodo(dir.path(), &["do", "1"]);
    assert_eq!(tasks(dir.path()), ["c", "d", "x:b", "x:a"]);
    let log =
        String::from_utf8_lossy(&txtodo(dir.path(), &["log", "-n", "50"]).stdout).into_owned();
    assert!(log.contains("you@"), "{log}");
    assert!(
        !log.contains("external@"),
        "the archive fell back to a whole-file write: {log}"
    );
}
