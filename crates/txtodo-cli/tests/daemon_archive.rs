//! Edits the CLI cannot say as plain mutations, against a real `txtodod`. `do` must reach the
//! daemon as one `Complete` per line (`complete_plan.rs`, in tagged and in the default sidecar
//! mode: the daemon moves the line itself); an explicit `archive` as guarded `MoveToEnd`s
//! (`archive_plan.rs`, tagged mode only: it needs `id:` tags in the text); anything else, like
//! dropping a blank line or any other sidecar-mode edit, as a `Replace` naming the hash it read.
//! None may be a direct write the daemon reconciles as an External edit, which can drop another
//! writer's concurrent `Apply`. Every change here goes through the socket, so `log` must show no
//! `external@` op.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::txtodod_binary;

/// How long to wait for the daemon socket.
const SOCKET_WAIT: Duration = Duration::from_secs(20);
/// Long enough for the daemon to notice and reconcile a direct disk write (150 ms debounce plus
/// file-watcher latency): only then would a fallback write show up in `log` as `external@`.
const RECONCILE_SETTLE: Duration = Duration::from_secs(2);

struct Daemon(Child);

impl Daemon {
    fn spawn(dir: &Path, identity_mode: &str) -> Daemon {
        let child = Command::new(txtodod_binary())
            .args([
                "--dir",
                &dir.to_string_lossy(),
                "--identity-mode",
                identity_mode,
            ])
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
        // The pretty stderr layer then carries `cli.mutation_plan`'s counts (`assert_one_complete`).
        .env("TXTODO_LOG", "debug")
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
    let _daemon = Daemon::spawn(dir.path(), "tagged");
    // Added through the daemon, so its history holds no External op (an adopted seed file would).
    for name in ["a", "b", "c", "d"] {
        txtodo(dir.path(), &["add", name]);
    }
    assert_one_complete(&txtodo(dir.path(), &["do", "1"]));
    assert_eq!(tasks(dir.path()), ["b", "c", "d", "x:a"]);
    // Task complete-to-bottom: `do` moves the line it completed and nothing else, so `b` lands
    // after `a`, which is already down there. (`do` used to run a full archive, which re-sorted
    // every done line into its old order and put `b` back above `a`.)
    assert_one_complete(&txtodo(dir.path(), &["do", "1"]));
    assert_eq!(tasks(dir.path()), ["c", "d", "x:a", "x:b"]);
    assert_all_through_the_socket(dir.path());
}

/// Root todo "daemon-mode do sends Edit plus MoveToEnd, so the op log never says complete": the
/// plan behind a `do` is exactly one mutation — the daemon's own `Complete`, which also moves the
/// line — not an `Edit` and a `MoveToEnd` (tagged) or a whole-document `Replace` (sidecar).
/// `$TXTODO_LOG=debug` puts `daemon_mode::log_mutation_plan`'s counts on stderr.
fn assert_one_complete(out: &Output) {
    let err = plain_stderr(out);
    assert!(
        err.contains("cli.mutation_plan expressible=true mutation_count=1"),
        "one Complete, not Edit + MoveToEnd or a Replace: {err}"
    );
}

/// The CLI's stderr minus the pretty layer's ANSI colour codes (`ESC [ ... m`), so a field reads
/// as `name=value`. https://en.wikipedia.org/wiki/ANSI_escape_code#CSI_(Control_Sequence_Introducer)_sequences
fn plain_stderr(out: &Output) -> String {
    let mut plain = String::new();
    let mut rest = String::from_utf8_lossy(&out.stderr).into_owned();
    while let Some(start) = rest.find("\x1b[") {
        plain.push_str(&rest[..start]);
        let after = &rest[start..];
        let end = after.find('m').map_or(after.len(), |m| m + 1);
        rest = after[end..].to_owned();
    }
    plain.push_str(&rest);
    plain
}

// CI-only: spawns a real txtodod (see `daemon_mode.rs`); run via `cargo test -- --ignored`.
#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[test]
fn do_in_sidecar_mode_is_one_complete_too() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path(), "sidecar");
    for name in ["a", "b", "c"] {
        txtodo(dir.path(), &["add", name]);
    }
    // Two lines at once: the second is addressed where it sits once the first has moved.
    assert_one_complete(&txtodo(dir.path(), &["do", "1"]));
    assert_eq!(tasks(dir.path()), ["b", "c", "x:a"]);
    let err = plain_stderr(&txtodo(dir.path(), &["do", "1", "2"]));
    assert!(err.contains("mutation_count=2"), "{err}");
    assert_eq!(tasks(dir.path()), ["x:a", "x:b", "x:c"]);
    assert_all_through_the_socket(dir.path());
}

fn log_of(dir: &Path) -> String {
    String::from_utf8_lossy(&txtodo(dir, &["log", "-n", "50"]).stdout).into_owned()
}

/// `txtodo log` must hold ops from this user and none from a reconciled disk write.
fn assert_all_through_the_socket(dir: &Path) {
    std::thread::sleep(RECONCILE_SETTLE);
    let log = log_of(dir);
    assert!(log.contains("you@"), "{log}");
    assert!(
        !log.contains("external@"),
        "a whole-file write reached the daemon as an external edit: {log}"
    );
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[test]
fn dropping_a_blank_line_is_a_guarded_replace_not_a_disk_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path(), "tagged");
    for name in ["a", "b", "c"] {
        txtodo(dir.path(), &["add", name]);
    }
    // `del` leaves a blank where `a` was; `archive` drops it, which no mutation can say.
    txtodo(dir.path(), &["del", "1"]);
    assert_eq!(tasks(dir.path()), ["", "b", "c"]);
    txtodo(dir.path(), &["archive"]);
    assert_eq!(tasks(dir.path()), ["b", "c"]);
    assert_all_through_the_socket(dir.path());
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[test]
fn sidecar_mode_edits_are_guarded_replaces_not_disk_writes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path(), "sidecar");
    txtodo(dir.path(), &["add", "call", "mum"]);
    txtodo(dir.path(), &["add", "walk", "dog"]);
    // No `id:` in sidecar text, so `pri` on a line that is not the last is a whole-line change no
    // mutation can address.
    txtodo(dir.path(), &["pri", "1", "A"]);
    // The last line used to read as "delete a line, append a line", which tombstoned the task and
    // minted a new one (its history gone, another device's concurrent edit to it refused). It must
    // stay the same task: a priority change, and no delete anywhere in the log yet.
    txtodo(dir.path(), &["pri", "2", "B"]);
    let log = log_of(dir.path());
    assert!(log.contains("Priority"), "{log}");
    assert!(
        !log.contains("Deleted"),
        "the task was tombstoned and re-added: {log}"
    );
    // `del` names its line by number alone, so it goes out led by a `RequireBase` on the hash the
    // command read (`base_guard.rs`); the real daemon must accept that hash while nothing moved.
    txtodo(dir.path(), &["del", "2"]);
    let text = std::fs::read_to_string(dir.path().join("todo.txt")).unwrap();
    assert!(text.starts_with("(A) "), "{text}");
    assert_eq!(
        text.lines().nth(1),
        Some(""),
        "`del` leaves a blank: {text:?}"
    );
    assert_all_through_the_socket(dir.path());
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[test]
fn an_add_after_a_trailing_blank_lands_where_direct_mode_puts_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path(), "sidecar");
    txtodo(dir.path(), &["add", "a"]);
    txtodo(dir.path(), &["add", "b"]);
    txtodo(dir.path(), &["del", "2"]);
    // The file is now `a` and a blank. Direct mode appends after the blank (line 3); the daemon
    // anchors an `Add` on the last task, which would put `c` on line 2 while `add` printed 3.
    let out = txtodo(dir.path(), &["add", "c"]);
    let printed = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(printed.starts_with("3 "), "{printed}");
    assert_eq!(tasks(dir.path()), ["a", "", "c"]);
    assert_all_through_the_socket(dir.path());
}
