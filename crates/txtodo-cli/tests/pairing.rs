//! `txtodo pair` end to end against two real `txtodod` daemons on the same machine, each its own
//! workspace and socket — plain unix-domain-socket IPC, so this needs none of `sync-lan-transport`
//! (the actual daemon-to-daemon leg, which has a known upstream iroh bug on loopback-literal
//! addresses per `tasks/sync-lan-transport/notes.md`, is not exercised here at all: see
//! `crates/txtodo-cli/src/commands/pair.rs`'s own module doc for why that leg cannot complete
//! today). What IS real and asserted here: the initiator's `PairOffer` QR/code render, the
//! joiner's `PairAccept`-derived SAS and explicit confirmation, `PairConfirmSas`, the post-confirm
//! workspace snapshot, and the identity_mode mismatch refusal (docs/questions.md Q6).
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const SOCKET_WAIT: Duration = Duration::from_secs(20);

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

/// Runs `txtodo` with no stdin (EOF): fine for the initiator, which never prompts.
fn txtodo(dir: &Path, config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", config)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo: {e}"))
}

/// Runs `txtodo` with `stdin_line` fed to its stdin, for the joiner's confirm prompt.
fn txtodo_with_stdin(dir: &Path, config: &Path, args: &[&str], stdin_line: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", config)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn txtodo: {e}"));
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin_line.as_bytes())
        .unwrap_or_else(|e| panic!("write stdin: {e}"));
    child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("wait: {e}"))
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Pulls the JSON code line out of `txtodo pair`'s own printed output: the line right after
/// "Code (...)": up to the blank line that follows it.
fn extract_code(offer_stdout: &str) -> String {
    let after = offer_stdout
        .split_once("other device):\n")
        .map(|(_, rest)| rest)
        .unwrap_or_else(|| panic!("no code line in: {offer_stdout}"));
    after
        .lines()
        .next()
        .unwrap_or_else(|| panic!("no code line in: {offer_stdout}"))
        .to_owned()
}

fn missing_config(dir: &Path) -> PathBuf {
    dir.join("none.toml")
}

#[test]
fn initiator_shows_a_qr_and_code_and_says_it_is_waiting() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), "").unwrap();
    let _daemon = Daemon::spawn(dir.path());

    let out = txtodo(dir.path(), &missing_config(dir.path()), &["pair"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("Code (no camera?"), "{text}");
    assert!(
        text.contains("network transport between two txtodo daemons has not landed"),
        "the known gap must be stated plainly, not hidden: {text}"
    );
    // A QR was actually rendered: full/half block glyphs, not just the JSON code text.
    assert!(
        text.contains('\u{2588}') || text.contains('\u{2584}'),
        "{text}"
    );
    let code = extract_code(&text);
    let value: serde_json::Value = serde_json::from_str(&code).unwrap();
    assert_eq!(value["identity_mode"], "sidecar", "{text}");
}

#[test]
fn joiner_sees_the_real_sas_confirms_and_gets_a_snapshot() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap();
    std::fs::write(dir_b.path().join("todo.txt"), "").unwrap();
    let _a = Daemon::spawn(dir_a.path());
    let _b = Daemon::spawn(dir_b.path());

    let offer = txtodo(dir_a.path(), &missing_config(dir_a.path()), &["pair"]);
    assert!(offer.status.success(), "{}", stderr(&offer));
    let code = extract_code(&stdout(&offer));

    let join = txtodo_with_stdin(
        dir_b.path(),
        &missing_config(dir_b.path()),
        &["pair", &code],
        "yes\n",
    );
    assert!(join.status.success(), "{}", stderr(&join));
    let text = stdout(&join);
    assert!(text.contains("Six words"), "{text}");
    assert!(text.contains("Confirmed on this device."), "{text}");
    assert!(text.contains("Workspace snapshot"), "{text}");
    assert!(text.contains("todo.txt"), "{text}");
    assert!(
        text.contains("still waiting on the initiator's own confirmation"),
        "{text}"
    );
}

#[test]
fn joiner_saying_no_aborts_without_confirming() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap();
    std::fs::write(dir_b.path().join("todo.txt"), "").unwrap();
    let _a = Daemon::spawn(dir_a.path());
    let _b = Daemon::spawn(dir_b.path());

    let offer = txtodo(dir_a.path(), &missing_config(dir_a.path()), &["pair"]);
    let code = extract_code(&stdout(&offer));

    let join = txtodo_with_stdin(
        dir_b.path(),
        &missing_config(dir_b.path()),
        &["pair", &code],
        "no\n",
    );
    assert!(!join.status.success(), "a declined SAS must not succeed");
    assert!(stderr(&join).contains("aborted"), "{}", stderr(&join));
}

#[test]
fn a_detected_identity_mode_mismatch_against_a_non_empty_workspace_is_refused() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap();
    // B already has a task: nothing empty to safely adopt a foreign mode into (Q6).
    std::fs::write(dir_b.path().join("todo.txt"), "(A) existing task\n").unwrap();
    let _a = Daemon::spawn(dir_a.path()); // sidecar, the daemon default
    let _b = Daemon::spawn(dir_b.path());

    let offer = txtodo(dir_a.path(), &missing_config(dir_a.path()), &["pair"]);
    let code = extract_code(&stdout(&offer));

    // B's own CLI config asks for tagged mode, disagreeing with A's sidecar offer.
    let b_config = dir_b.path().join("config.toml");
    std::fs::write(&b_config, "identity_mode = \"tagged\"\n").unwrap();

    let join = txtodo_with_stdin(dir_b.path(), &b_config, &["pair", &code], "yes\n");
    assert!(
        !join.status.success(),
        "a real mismatch against existing tasks must refuse, never guess"
    );
    let err = stderr(&join);
    assert!(err.contains("identity_mode"), "{err}");
    assert!(err.contains("Q6"), "{err}");
}
