//! tasks/test-nested-ref-sync/notes.md, the part that needs no live network sync: `todo.sh -d`
//! reads and writes a `ref:` sub-list `txtodo sub` created, agreeing with `txtodo sub ... ls`
//! (specs/ref-directories.md rule 12). The other half of that task — syncing a nested-ref
//! workspace to a fresh device — needs two real `txtodod` processes over the LAN transport, which
//! is blocked by the upstream iroh/noq-proto bug documented in
//! tasks/sync-lan-transport/notes.md; not attempted here.
//!
//! Real `txtodod` + the vendored todo.sh, same harness shapes as `tests/daemon_mode.rs` and
//! `tests/todosh_parity.rs` — copied, not shared (constitution §7).
#![cfg(not(windows))]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const SOCKET_WAIT: Duration = Duration::from_secs(20);
const TODO_SH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vendor/todo.sh");

fn txtodod_binary() -> PathBuf {
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

/// `EDITOR` is a no-op: `notes` must not need a real interactive editor to lazily create the
/// `ref:` directory (the tag+directory land before the editor even opens, per
/// cli-ref-commands/notes.md).
fn txtodo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .env("EDITOR", "true")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo: {e}"))
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The `ref:` slug on todo.txt's first line, once `notes` has created it.
fn ref_slug(dir: &Path) -> String {
    let text = std::fs::read_to_string(dir.join("todo.txt")).unwrap_or_default();
    text.lines()
        .next()
        .and_then(|l| l.split_whitespace().find_map(|w| w.strip_prefix("ref:")))
        .unwrap_or_else(|| panic!("no ref: tag in {text:?}"))
        .to_owned()
}

fn todo_sh_cfg(ref_dir: &Path) -> PathBuf {
    let cfg = format!(
        "export TODO_DIR=\"{d}\"\nexport TODO_FILE=\"$TODO_DIR/todo.txt\"\nexport DONE_FILE=\"$TODO_DIR/done.txt\"\nexport REPORT_FILE=\"$TODO_DIR/report.txt\"\n",
        d = ref_dir.display()
    );
    let path = ref_dir.join("todo.cfg");
    std::fs::write(&path, cfg).unwrap_or_else(|e| panic!("{e}"));
    path
}

fn run_todo_sh(ref_dir: &Path, cfg: &Path, step: &[&str]) -> Output {
    Command::new(TODO_SH)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", ref_dir)
        .current_dir(ref_dir)
        .args(["-d", cfg.to_str().unwrap(), "-f", "-t", "-p"])
        .args(step)
        .output()
        .unwrap_or_else(|e| panic!("todo.sh: {e}"))
}

/// Adds a task at the workspace root and lazily creates its `ref:` directory via `notes` (no real
/// interactive editor needed — the tag+directory land before the editor even opens). Returns it.
fn create_ref_dir(root: &Path) -> PathBuf {
    let add = txtodo(root, &["add", "Q4 roadmap"]);
    assert!(add.status.success(), "{}", stderr(&add));
    let notes = txtodo(root, &["notes", "1"]);
    assert!(notes.status.success(), "{}", stderr(&notes));
    let ref_dir = root.join(ref_slug(root));
    assert!(ref_dir.is_dir(), "{}", ref_dir.display());
    ref_dir
}

#[test]
fn todo_sh_d_reads_a_sub_list_txtodo_sub_created_and_agrees_with_txtodo_sub() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(root.path());
    let ref_dir = create_ref_dir(root.path());

    // `sub` writes into the sub-list like any other `txtodo add` (direct-file mode there, since
    // only the workspace root runs a daemon — rule 12's "other tools see an inert tag").
    let sub_add = txtodo(root.path(), &["sub", "1", "add", "leaf task"]);
    assert!(sub_add.status.success(), "{}", stderr(&sub_add));
    let sub_todo = std::fs::read_to_string(ref_dir.join("todo.txt")).unwrap_or_default();
    assert!(sub_todo.contains("leaf task"), "{sub_todo}");

    // todo.sh -d <ref>/todo.cfg sees the same file like any other todo.txt (rule 12).
    let cfg = todo_sh_cfg(&ref_dir);
    let sh_ls = run_todo_sh(&ref_dir, &cfg, &["ls"]);
    assert!(sh_ls.status.success(), "{}", stderr(&sh_ls));
    assert!(stdout(&sh_ls).contains("leaf task"), "{}", stdout(&sh_ls));

    // Agreement: `txtodo sub 1 ls` lists the very same line.
    let sub_ls = txtodo(root.path(), &["sub", "1", "ls"]);
    assert!(sub_ls.status.success(), "{}", stderr(&sub_ls));
    assert!(stdout(&sub_ls).contains("leaf task"), "{}", stdout(&sub_ls));

    // A line todo.sh -d adds is in turn visible to `txtodo sub ... ls` (round trip both ways).
    let sh_add = run_todo_sh(&ref_dir, &cfg, &["add", "todo.sh line"]);
    assert!(sh_add.status.success(), "{}", stderr(&sh_add));
    let sub_ls_2 = txtodo(root.path(), &["sub", "1", "ls"]);
    assert!(sub_ls_2.status.success(), "{}", stderr(&sub_ls_2));
    assert!(
        stdout(&sub_ls_2).contains("todo.sh line"),
        "{}",
        stdout(&sub_ls_2)
    );
}
