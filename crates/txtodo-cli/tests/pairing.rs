//! `txtodo pair` end to end against two real `txtodod` daemons, each its own workspace and
//! socket. Every real `txtodod` runs the real LAN transport unconditionally (`main.rs`'s own
//! `lan::start`), so unlike an earlier pass of this test, pairing now genuinely crosses the
//! network: the initiator's `txtodo pair` blocks on `PairAwaitPeer` until a real joiner connects
//! (plan M4 `sync-pairing`'s LAN wiring pass, `crates/txtodo-daemon/src/pairing_lan.rs`), so it
//! must always be spawned in the background here rather than run to completion with `.output()`.
//! What's asserted: the initiator's `PairOffer` QR/code render, the joiner's real
//! `PairAccept`-derived SAS matching the initiator's own real `PairAwaitPeer`-derived SAS, both
//! sides' explicit confirmation, a declined SAS never confirming, and the identity_mode mismatch
//! refusal (docs/questions.md Q6) never even reaching the network.
//!
//! [`a_paired_joiner_receives_the_initiators_real_file`] asserts the other half: the joiner then
//! actually receives the initiator's file content over the LAN. That required
//! `tasks/pairing-workspace-identity/` — the initiator's real `WorkspaceId` now rides the pairing
//! code, and the joiner adopts it, so both sides route post-pairing sync messages to the same id.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Output, Stdio};
use std::time::{Duration, Instant};

const SOCKET_WAIT: Duration = Duration::from_secs(20);
/// Generous: real mDNS discovery plus the pairing relay's own retry burst
/// (`pairing_lan.rs::RETRY_INTERVAL`) typically finishes in a few seconds
/// (`lan_loopback_converge.rs`'s own measurements), but a shared CI runner can be slow.
const PAIR_CONVERGE_WAIT: Duration = Duration::from_secs(30);

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

/// Runs `txtodo` to completion with `stdin_line` fed to its stdin, for the joiner's confirm
/// prompt (which the joiner side never blocks past, so `.output()` is fine here).
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

/// Spawns `txtodo pair` (the initiator) in the background — it now blocks on a real peer
/// (`PairAwaitPeer`), so it can never be run with `.output()` in a single-device test. `stdin_line`
/// is written immediately so it is already buffered by the time the SAS confirm prompt reads it.
/// Returns the child (to `wait()` on later) and a reader positioned right after the code line.
fn spawn_pair_offer(
    dir: &Path,
    config: &Path,
    stdin_line: &str,
) -> (Child, BufReader<ChildStdout>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", config)
        .args(["pair"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn txtodo pair: {e}"));
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin_line.as_bytes())
        .unwrap_or_else(|e| panic!("write stdin: {e}"));
    let stdout = child.stdout.take().expect("piped stdout");
    (child, BufReader::new(stdout))
}

/// Reads lines from a still-running `txtodo pair`'s stdout until the code line (the one right
/// after the "...other device):" marker) appears, and returns everything read so far plus the
/// code itself — the process keeps running past this point (see `spawn_pair_offer`'s doc).
fn read_until_code(reader: &mut BufReader<ChildStdout>) -> (String, String) {
    let mut seen = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .unwrap_or_else(|e| panic!("read txtodo pair stdout: {e}"));
        assert!(n > 0, "txtodo pair exited before printing a code:\n{seen}");
        seen.push_str(&line);
        if seen.ends_with("other device):\n") {
            line.clear();
            reader
                .read_line(&mut line)
                .unwrap_or_else(|e| panic!("read code line: {e}"));
            seen.push_str(&line);
            return (seen, line.trim_end().to_owned());
        }
    }
}

/// Reads the rest of a `txtodo pair` child's stdout until it exits, and waits for it.
fn finish_pair_offer(mut child: Child, mut reader: BufReader<ChildStdout>) -> (bool, String) {
    let mut rest = String::new();
    reader
        .read_to_string(&mut rest)
        .unwrap_or_else(|e| panic!("read remaining stdout: {e}"));
    let status = child.wait().unwrap_or_else(|e| panic!("wait: {e}"));
    (status.success(), rest)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn missing_config(dir: &Path) -> PathBuf {
    dir.join("none.toml")
}

/// Polls `path`'s bytes against `want` until they match, or [`PAIR_CONVERGE_WAIT`] elapses.
fn wait_for_file_convergence(path: &Path, want: &[u8]) {
    let start = Instant::now();
    loop {
        if std::fs::read(path).ok().as_deref() == Some(want) {
            return;
        }
        assert!(
            start.elapsed() < PAIR_CONVERGE_WAIT,
            "{} did not converge to the initiator's content within {PAIR_CONVERGE_WAIT:?}: got {:?}",
            path.display(),
            std::fs::read(path)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned()),
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The compact code's nine fields, in the exact order `commands::pair::PairingCode` declares them
/// — postcard is not self-describing like JSON, so decoding this way only works when the field
/// order matches exactly. Duplicated here rather than shared: `pair.rs`'s own struct is a private
/// `fn`-module item, unreachable from this separate integration-test binary. `#[allow(dead_code)]`
/// on the trailing fields: `serde` needs them declared to consume their bytes even though this
/// test only reads `identity_mode`.
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct DecodedCode {
    device: String,
    group_id: String,
    x25519_pub: String,
    endpoint: String,
    nonce: String,
    identity_mode: String,
    relay_node_id: String,
    relay_url: String,
    workspace_id: String,
}

/// Decodes `commands::pair::to_compact`'s own postcard-then-base32 text.
fn decode_compact_code(code: &str) -> DecodedCode {
    let bytes = data_encoding::BASE32_NOPAD
        .decode(code.trim().to_ascii_uppercase().as_bytes())
        .unwrap_or_else(|e| panic!("code is not valid base32: {e}\n{code}"));
    postcard::from_bytes(&bytes)
        .unwrap_or_else(|e| panic!("code is not valid postcard: {e}\n{code}"))
}

/// Asserts the QR/code preamble an initiator's `txtodo pair` prints before it ever sees a peer.
/// The printed text fallback is the compact code (task `pairing-code-compact`); the QR itself
/// still renders JSON underneath (unchanged), which is why the glyph check below stays separate
/// from decoding `code`.
fn assert_offer_preamble(seen: &str, code: &str) {
    assert!(seen.contains("Code (no camera?"), "{seen}");
    assert!(
        seen.contains('\u{2588}') || seen.contains('\u{2584}'),
        "a QR must actually render: {seen}"
    );
    let decoded = decode_compact_code(code);
    assert_eq!(decoded.identity_mode, "sidecar", "{code}");
}

/// Asserts both sides confirmed for real once a handshake completes — same wording, one side
/// prints "Paired." (the joiner, once the group key lands) and the other does not.
fn assert_confirmed(text: &str) {
    assert!(text.contains("Six words"), "{text}");
    assert!(text.contains("Confirmed on this device."), "{text}");
}

/// What a completed ceremony leaves behind, all of it kept alive for the caller: dropping either
/// [`Daemon`] kills its `txtodod`, and dropping either `TempDir` deletes that workspace.
struct PairedDevices {
    _a: Daemon,
    _b: Daemon,
    _dir_a: tempfile::TempDir,
    dir_b: tempfile::TempDir,
}

/// Runs one real `txtodo pair` ceremony between two fresh daemons and asserts every step of it:
/// the initiator's QR/code preamble, the joiner's real SAS confirm and "Paired.", and the
/// initiator's own confirm once the joiner has answered. `a_todo` seeds the initiator's todo.txt;
/// the joiner's starts empty.
fn pair_two_real_devices(a_todo: &str) -> PairedDevices {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), a_todo).unwrap();
    std::fs::write(dir_b.path().join("todo.txt"), "").unwrap();
    let a = Daemon::spawn(dir_a.path());
    let b = Daemon::spawn(dir_b.path());

    let (offer_child, mut offer_reader) =
        spawn_pair_offer(dir_a.path(), &missing_config(dir_a.path()), "yes\n");
    let (offer_seen_so_far, code) = read_until_code(&mut offer_reader);
    assert_offer_preamble(&offer_seen_so_far, &code);

    let join = txtodo_with_stdin(
        dir_b.path(),
        &missing_config(dir_b.path()),
        &["pair", &code],
        "yes\n",
    );
    assert!(join.status.success(), "{}", stderr(&join));
    let join_text = stdout(&join);
    assert_confirmed(&join_text);
    assert!(join_text.contains("Paired."), "{join_text}");

    let (offer_ok, offer_rest) = finish_pair_offer(offer_child, offer_reader);
    assert!(
        offer_ok,
        "initiator's own pair exited non-zero: {offer_rest}"
    );
    assert_confirmed(&offer_rest);

    PairedDevices {
        _a: a,
        _b: b,
        _dir_a: dir_a,
        dir_b,
    }
}

#[test]
fn two_real_devices_complete_a_real_pairing_ceremony() {
    let _paired = pair_two_real_devices("(A) buy milk id:01M2CZ00000000000000000A\n");
}

/// This is the only test in the repo that pairs two *genuinely independent* daemons (every other
/// real-daemon pairing test — `txtodo-daemon`'s `pairing_lan.rs`, `pairing_relay.rs`,
/// `relay_multiplex.rs` — pre-seeds both sides with the same `WorkspaceId` via
/// `Daemon::start_with_workspace_id`/`seed_workspace_at`) and then asks whether sync actually
/// converges. It was quarantined 2026-09-16 (`tasks/pairing-workspace-identity/`) because `txtodo
/// pair` agreed a group id/key but never a workspace id, so `lan_session_dispatch.rs` dropped
/// every post-pairing sync message as unrouted. Fixed by carrying the initiator's real
/// `WorkspaceId` on the pairing code (`PairOfferResponse.workspace_id`) and having the joiner
/// adopt it (`WorkspaceCatalog::adopt_offered_workspace_id`, first-registrant-wins) on
/// `PairAccept` — this test is that fix's acceptance bar.
#[test]
fn a_paired_joiner_receives_the_initiators_real_file() {
    let todo_content = "(A) buy milk id:01M2CZ00000000000000000A\n";
    let paired = pair_two_real_devices(todo_content);
    // The real acceptance bar: B's own disk file now holds A's real content, delivered over the
    // real LAN transport (lan.rs's existing group-keyed sync engine) once pairing adopted a shared
    // group id and key — not merely B's own pre-existing (empty) file.
    wait_for_file_convergence(
        &paired.dir_b.path().join("todo.txt"),
        todo_content.as_bytes(),
    );
}

#[test]
fn joiner_saying_no_aborts_without_confirming_while_the_initiator_still_confirms_its_own_side() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap();
    std::fs::write(dir_b.path().join("todo.txt"), "").unwrap();
    let _a = Daemon::spawn(dir_a.path());
    let _b = Daemon::spawn(dir_b.path());

    let (offer_child, mut offer_reader) =
        spawn_pair_offer(dir_a.path(), &missing_config(dir_a.path()), "yes\n");
    let (_seen, code) = read_until_code(&mut offer_reader);

    let join = txtodo_with_stdin(
        dir_b.path(),
        &missing_config(dir_b.path()),
        &["pair", &code],
        "no\n",
    );
    assert!(!join.status.success(), "a declined SAS must not succeed");
    assert!(stderr(&join).contains("aborted"), "{}", stderr(&join));

    // A's own handshake still completed for real (B's PairAccept reached it over the LAN before B
    // declined) and A's own "yes" still confirms its side — a joiner declining is not a network
    // failure on the initiator's end.
    let (offer_ok, offer_rest) = finish_pair_offer(offer_child, offer_reader);
    assert!(
        offer_ok,
        "initiator's own pair exited non-zero: {offer_rest}"
    );
    assert_confirmed(&offer_rest);
}

#[test]
fn a_detected_identity_mode_mismatch_against_a_non_empty_workspace_is_refused_before_any_network_call()
 {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("todo.txt"), "").unwrap();
    // B already has a task: nothing empty to safely adopt a foreign mode into (Q6).
    std::fs::write(dir_b.path().join("todo.txt"), "(A) existing task\n").unwrap();
    let _a = Daemon::spawn(dir_a.path()); // sidecar, the daemon default
    let _b = Daemon::spawn(dir_b.path());

    let (mut offer_child, mut offer_reader) =
        spawn_pair_offer(dir_a.path(), &missing_config(dir_a.path()), "yes\n");
    let (_seen, code) = read_until_code(&mut offer_reader);

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

    // B never called PairAccept at all (the refusal is a local check, before any RPC), so A never
    // sees a peer; its own pair would otherwise sit waiting the full PAIRING_WINDOW_MS, so this
    // test ends it directly rather than waiting that out.
    let _ = offer_child.kill();
    let _ = offer_child.wait();
}
