//! `txtodo workspace add|remove|list` and the CLI's global-socket fallback end to end (ADR 0025,
//! task `cli-workspace-commands`): a real `txtodod` in TRUE GLOBAL mode (`--dir` omitted), hermetic
//! via `$TXTODO_SOCKET`, with no per-directory socket anywhere — so every call here only succeeds
//! if `client::select` actually falls through to the global daemon and attaches a real `Path`
//! selector, not the pre-existing `--dir`-bridge path `daemon_mode.rs` already covers.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::txtodod_binary;

const SOCKET_WAIT: Duration = Duration::from_secs(20);

/// A real `txtodod` in true global mode (`--dir` omitted), hermetic via `$TXTODO_SOCKET`/
/// `$TXTODO_REGISTRY_DB` so it never touches this machine's real `$XDG_DATA_HOME/txtodo/`.
struct GlobalDaemon {
    child: Child,
    socket: PathBuf,
}

impl GlobalDaemon {
    fn spawn(state_dir: &Path) -> GlobalDaemon {
        let socket = state_dir.join("txtodod.sock");
        let child = Command::new(txtodod_binary())
            .env("TXTODO_SOCKET", &socket)
            .env("TXTODO_REGISTRY_DB", state_dir.join("registry.db"))
            // Debug-only seam (`identity_setup.rs`): a freshly rebuilt `txtodod` otherwise blocks
            // in the OS keychain prompt on a developer Mac, past `SOCKET_WAIT`.
            .env("TXTODO_TEST_KEYSTORE_MEMORY", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn txtodod: {e}"));
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < SOCKET_WAIT,
                "daemon socket did not appear"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        GlobalDaemon { child, socket }
    }
}

impl Drop for GlobalDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs `txtodo` against `daemon`'s socket, from `dir` — which must have no `.txtodo/txtodod.sock`
/// of its own, so `client::select` has nothing to fall back to except the global daemon.
fn txtodo(daemon: &GlobalDaemon, dir: &Path, args: &[&str]) -> Output {
    assert!(
        !dir.join(".txtodo").join("txtodod.sock").exists(),
        "this harness only proves the global-daemon path"
    );
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .env("TXTODO_SOCKET", &daemon.socket)
        // A test daemon that is not up must fail this test, never make the CLI spawn a daemon
        // of its own at this socket with the machine's real registry.
        .env("TXTODO_NO_AUTOSTART", "1")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo: {e}"))
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `doctor` against a test daemon: the in-memory keystore row is the one FAIL allowed (see the
/// call site); anything else failing is a real regression and the whole report is the message.
fn assert_only_keystore_fails(doctor_out: &str) {
    let other_fails: Vec<&str> = doctor_out
        .lines()
        .filter(|l| l.contains(" FAIL ") && !l.starts_with("keystore"))
        .collect();
    assert!(
        other_fails.is_empty(),
        "unexpected FAIL rows {other_fails:?} in:\n{doctor_out}"
    );
}

/// The `workspace list` rows other than the default workspace, which every global daemon now
/// creates and registers on its own (task default-workspace).
fn non_default_rows(list: &str) -> Vec<&str> {
    list.lines().filter(|l| !l.contains("[default]")).collect()
}

#[test]
fn add_remove_list_round_trip_and_add_is_idempotent() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws_dir = tempfile::tempdir().unwrap();
    std::fs::write(ws_dir.path().join("todo.txt"), "").unwrap();

    let empty = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    assert!(empty.status.success(), "{}", stdout(&empty));
    assert!(
        non_default_rows(&stdout(&empty)).is_empty(),
        "{}",
        stdout(&empty)
    );
    assert!(
        stdout(&empty).contains("[default]"),
        "the default is listed: {}",
        stdout(&empty)
    );

    let add = txtodo(&daemon, ws_dir.path(), &["workspace", "add"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let added = stdout(&add);
    let id = added
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("no id in: {added}"))
        .to_owned();

    // Idempotent: adding the same directory again returns the same id, not a duplicate entry.
    let add_again = txtodo(&daemon, ws_dir.path(), &["workspace", "add"]);
    assert!(
        stdout(&add_again).starts_with(&id),
        "{}",
        stdout(&add_again)
    );
    let list = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    assert_eq!(
        non_default_rows(&stdout(&list)).len(),
        1,
        "still one entry: {}",
        stdout(&list)
    );

    let remove = txtodo(&daemon, ws_dir.path(), &["workspace", "remove", &id]);
    assert!(remove.status.success(), "{}", stdout(&remove));
    let after = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    assert!(
        non_default_rows(&stdout(&after)).is_empty(),
        "{}",
        stdout(&after)
    );
}

#[test]
fn a_todo_command_with_no_per_dir_socket_reaches_the_global_daemon() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws_dir = tempfile::tempdir().unwrap();
    std::fs::write(ws_dir.path().join("todo.txt"), "").unwrap();

    // No `workspace add` first: the Path selector auto-registers this directory, the same bridge
    // `WorkspaceCatalog::resolve` already proves at the daemon level (workspace_catalog_tests.rs).
    let add = txtodo(&daemon, ws_dir.path(), &["add", "buy", "milk"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(stdout(&add).contains("TODO: 1 added."), "{}", stdout(&add));

    let log = txtodo(&daemon, ws_dir.path(), &["log"]);
    assert!(log.status.success(), "{}", stdout(&log));
    assert!(
        stdout(&log).contains("you@"),
        "the add reached the daemon, not direct-file mode: {}",
        stdout(&log)
    );

    let list = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    assert_eq!(
        non_default_rows(&stdout(&list)).len(),
        1,
        "auto-registered exactly once: {}",
        stdout(&list)
    );
}

/// todo `ref:cli-workspace-autoregister`: a completely fresh directory — no `todo.txt`, no
/// `.txtodo/`, nothing — with a global daemon reachable and no separate `workspace add` step.
/// `daemon_mode::run_via_daemon`'s very first call (`list_files`) already carries the same Path
/// selector every other RPC does (`client.rs::select`), so this needs no new production code —
/// only this regression test, guarding the behavior `cli-workspace-commands` already delivered.
#[test]
fn first_add_in_a_brand_new_directory_auto_registers_it_with_no_separate_step() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws_dir = tempfile::tempdir().unwrap();
    assert!(!ws_dir.path().join("todo.txt").exists());

    // Named with --dir: a folder with no todo.txt is no workspace, so with no --dir this would
    // land in the default workspace instead (see the test below).
    let dir_arg = ws_dir.path().display().to_string();
    let add = txtodo(
        &daemon,
        ws_dir.path(),
        &["--dir", &dir_arg, "add", "first", "task"],
    );
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(stdout(&add).contains("TODO: 1 added."), "{}", stdout(&add));
    assert!(
        ws_dir.path().join("todo.txt").exists(),
        "add creates a missing todo.txt, same as todo.sh"
    );

    let list = txtodo(&daemon, ws_dir.path(), &["workspace", "list"]);
    let out = stdout(&list);
    assert_eq!(
        non_default_rows(&out).len(),
        1,
        "auto-registered, no manual step: {out}"
    );
    assert!(out.contains(&ws_dir.path().display().to_string()), "{out}");
}

/// Task default-workspace: outside any workspace and with no --dir, a command lands in the default
/// workspace and says so, instead of registering the folder it happens to run in.
#[test]
fn a_command_outside_any_workspace_lands_in_the_default_and_says_so() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let elsewhere = tempfile::tempdir().unwrap();

    let add = txtodo(&daemon, elsewhere.path(), &["add", "goes", "to", "default"]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let notice = String::from_utf8_lossy(&add.stderr).into_owned();
    assert!(notice.contains("using the default workspace"), "{notice}");
    assert!(
        !elsewhere.path().join("todo.txt").exists(),
        "nothing written where it ran"
    );

    let default_todo = state_dir.path().join("default").join("todo.txt");
    assert!(
        std::fs::read_to_string(default_todo)
            .unwrap()
            .contains("goes to default")
    );
    let list = stdout(&txtodo(&daemon, elsewhere.path(), &["workspace", "list"]));
    assert!(
        non_default_rows(&list).is_empty(),
        "the folder was not registered: {list}"
    );
}

/// Registers `alive`/`dead` (both real dirs at the time of `add`), then removes `dead`'s directory
/// from disk so its registration's `root_exists` goes false — the fixture both prune tests share.
fn register_one_alive_one_dead(
    daemon: &GlobalDaemon,
    alive: &Path,
    dead: &tempfile::TempDir,
) -> PathBuf {
    std::fs::write(alive.join("todo.txt"), "").unwrap();
    std::fs::write(dead.path().join("todo.txt"), "").unwrap();
    txtodo(daemon, alive, &["workspace", "add"]);
    txtodo(daemon, dead.path(), &["workspace", "add"]);
    dead.path().to_path_buf()
}

/// tasks/test-registry-leak-cleanup: `workspace prune` (no `--yes`) lists only the dead
/// registration, names it by its now-gone path, never lists the still-real one, and changes
/// nothing in the registry.
#[test]
fn prune_dry_run_lists_only_dead_registrations_and_changes_nothing() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let alive_dir = tempfile::tempdir().unwrap();
    let dead_dir = tempfile::tempdir().unwrap();
    let dead_path = register_one_alive_one_dead(&daemon, alive_dir.path(), &dead_dir);
    drop(dead_dir); // the registered root no longer exists on disk from here on

    let out = stdout(&txtodo(&daemon, alive_dir.path(), &["workspace", "prune"]));
    assert!(out.contains(&dead_path.display().to_string()), "{out}");
    assert!(
        !out.contains(&alive_dir.path().display().to_string()),
        "the live workspace must never be listed as prune-worthy: {out}"
    );
    assert!(out.contains("--yes"), "dry run by default: {out}");

    let list = stdout(&txtodo(&daemon, alive_dir.path(), &["workspace", "list"]));
    assert_eq!(
        non_default_rows(&list).len(),
        2,
        "dry run changed nothing: {list}"
    );
}

/// tasks/test-registry-leak-cleanup: `workspace prune --yes` removes exactly the dead
/// registration, leaving the still-real one in the registry.
#[test]
fn prune_yes_removes_only_dead_registrations() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let alive_dir = tempfile::tempdir().unwrap();
    let dead_dir = tempfile::tempdir().unwrap();
    register_one_alive_one_dead(&daemon, alive_dir.path(), &dead_dir);
    drop(dead_dir);

    txtodo(&daemon, alive_dir.path(), &["workspace", "prune", "--yes"]);

    let out = stdout(&txtodo(&daemon, alive_dir.path(), &["workspace", "list"]));
    assert_eq!(
        non_default_rows(&out).len(),
        1,
        "only the dead one removed: {out}"
    );
    assert!(
        out.contains(&alive_dir.path().display().to_string()),
        "{out}"
    );
}

/// todo `ref:cli-doctor-multi-workspace`: `txtodo doctor` from one workspace reports every *other*
/// registered workspace too, not just the cwd's — one `workspace` row per entry, alongside the
/// seven fixed checks the cwd's own workspace already gets in full depth.
#[test]
fn doctor_reports_every_other_registered_workspace() {
    let state_dir = tempfile::tempdir().unwrap();
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap();
    std::fs::write(dir_b.path().join("todo.txt"), "").unwrap();

    assert!(
        txtodo(&daemon, dir_a.path(), &["add", "in a"])
            .status
            .success()
    );
    assert!(
        txtodo(&daemon, dir_b.path(), &["add", "in b"])
            .status
            .success()
    );

    let doctor_a = txtodo(&daemon, dir_a.path(), &["doctor"]);
    let out = stdout(&doctor_a);
    // The test daemon runs on the in-memory keystore (`.cargo/config.toml`'s
    // `TXTODO_TEST_KEYSTORE_MEMORY`), which `doctor` deliberately FAILs (task relay-id-keystore),
    // so exit 1 is expected here; that row must be the only FAIL, and stdout names any other.
    assert_only_keystore_fails(&out);
    // The default workspace is one more registered workspace; this test is about a and b.
    let workspace_rows: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("workspace") && !l.contains("/default"))
        .collect();
    assert_eq!(
        workspace_rows.len(),
        1,
        "exactly one other-workspace row (b, not a): {out}"
    );
    assert!(
        workspace_rows[0].contains(&dir_b.path().display().to_string()),
        "the row names b, not a: {workspace_rows:?}"
    );
}
