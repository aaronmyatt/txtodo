//! Daemon mode end to end: a real `txtodod` on a temp workspace, then `txtodo add`/`pri`/`del` go
//! through the socket and `log` shows User ops; `--no-daemon` writes the file directly and the
//! daemon records the edit as External. The daemon binary is built on demand with cargo because
//! this crate may not depend on txtodo-daemon (slice rule); the socket is the boundary.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// How long to wait for the daemon socket.
const SOCKET_WAIT: Duration = Duration::from_secs(20);
/// How long to wait for the daemon to notice a direct write (debounce + FSEvents latency).
const RECONCILE_WAIT: Duration = Duration::from_secs(8);

fn txtodod_binary() -> PathBuf {
    // target/debug/deps/<test> → target/debug/txtodod; build it if this run only built the CLI.
    let mut dir = std::env::current_exe().unwrap_or_else(|e| panic!("current_exe: {e}"));
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    let bin = dir.join(format!("txtodod{}", std::env::consts::EXE_SUFFIX));
    if !bin.exists() {
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "txtodo-daemon",
                "--bin",
                "txtodod",
                "--quiet",
            ])
            .status()
            .unwrap_or_else(|e| panic!("cargo build txtodod: {e}"));
        assert!(status.success(), "building txtodod failed");
    }
    assert!(bin.exists(), "{}", bin.display());
    bin
}

struct Daemon {
    child: Child,
}

impl Daemon {
    fn spawn(dir: &Path) -> Daemon {
        let child = Command::new(txtodod_binary())
            .args(["--dir", &dir.to_string_lossy()])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
        let socket = dir.join(".txtodo").join("txtodod.sock");
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < SOCKET_WAIT,
                "daemon socket did not appear"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Daemon { child }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn txtodo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo: {e}"))
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn todo_txt(dir: &Path) -> String {
    String::from_utf8(std::fs::read(dir.join("todo.txt")).unwrap_or_default()).unwrap_or_default()
}

/// Polls `txtodo log` until `pred` holds; the daemon learns of direct writes through its watcher.
fn wait_for_log(dir: &Path, pred: impl Fn(&str) -> bool) -> String {
    let start = Instant::now();
    loop {
        let log = stdout(&txtodo(dir, &["log", "-n", "10"]));
        if pred(&log) {
            return log;
        }
        assert!(
            start.elapsed() < RECONCILE_WAIT,
            "daemon did not reconcile in time: {log}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn add_and_pri_go_through_the_daemon() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path());
    let out = txtodo(dir.path(), &["add", "(B)", "call", "mum", "@phone"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout(&out).contains("TODO: 1 added."), "{}", stdout(&out));
    let text = todo_txt(dir.path());
    assert!(text.starts_with("(B) "), "{text}");
    assert!(text.contains("call mum @phone id:"), "{text}");
    assert!(txtodo(dir.path(), &["pri", "1", "A"]).status.success());
    assert!(
        todo_txt(dir.path()).starts_with("(A) "),
        "pri went through the daemon"
    );
}

#[test]
fn del_leaves_a_blank_and_log_shows_only_user_ops() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path());
    assert!(txtodo(dir.path(), &["add", "first"]).status.success());
    assert!(txtodo(dir.path(), &["add", "second"]).status.success());
    assert!(txtodo(dir.path(), &["del", "1"]).status.success());
    let text = todo_txt(dir.path());
    assert!(
        text.starts_with('\n'),
        "todo.sh del leaves a blank line: {text:?}"
    );
    let log = txtodo(dir.path(), &["log"]);
    assert!(
        log.status.success(),
        "{}",
        String::from_utf8_lossy(&log.stderr)
    );
    let log = stdout(&log);
    assert!(log.contains("you@"), "{log}");
    assert!(
        !log.contains("external@"),
        "every change came through the socket: {log}"
    );
}

#[test]
fn a_direct_write_is_reconciled_as_an_external_edit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path());
    assert!(txtodo(dir.path(), &["add", "first"]).status.success());
    let direct = txtodo(dir.path(), &["--no-daemon", "add", "typed", "with", "vim"]);
    assert!(direct.status.success());
    assert!(
        todo_txt(dir.path()).contains("typed with vim"),
        "the direct write landed on disk"
    );
    let log = wait_for_log(dir.path(), |l| l.contains("external@"));
    assert!(
        log.contains("external@"),
        "the direct write was reconciled as an external edit: {log}"
    );
}

#[test]
fn history_commands_without_a_daemon_say_so() {
    let dir = tempfile::tempdir().unwrap();
    let out = txtodo(dir.path(), &["log"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs the daemon"));
}

#[test]
fn conflicts_without_a_daemon_say_so() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    // Both subcommands are daemon-only: a flag lives in the store, never in the file.
    for args in [
        &["conflicts"] as &[&str],
        &["conflicts", "resolve", "1", "mine"],
    ] {
        let out = txtodo(dir.path(), args);
        assert!(!out.status.success(), "{args:?} must fail");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("needs the daemon"),
            "{args:?}"
        );
    }
}

#[test]
fn conflicts_list_reports_none_and_resolve_names_the_missing_flag() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path());
    assert!(txtodo(dir.path(), &["add", "first"]).status.success());
    let out = txtodo(dir.path(), &["conflicts"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout(&out).contains("TODO: no conflicts."),
        "{}",
        stdout(&out)
    );
    // JSON mode prints nothing when there is nothing to report.
    let out = txtodo(dir.path(), &["conflicts", "--json"]);
    assert!(out.status.success());
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    // No flag has been raised (no sync has happened), so the daemon refuses the resolve.
    let before = todo_txt(dir.path());
    let out = txtodo(dir.path(), &["conflicts", "resolve", "1", "merged"]);
    assert!(!out.status.success(), "resolve without a flag must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no open needs_review flag"), "{err}");
    assert_eq!(
        before,
        todo_txt(dir.path()),
        "a refused resolve writes nothing"
    );
}
