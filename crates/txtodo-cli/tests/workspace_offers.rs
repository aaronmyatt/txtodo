//! `txtodo workspace offers|accept|decline` end to end (task `workspace-offer-cli`) against a real
//! global `txtodod`. No peer is paired here, so the pending set is empty: this proves the three
//! commands reach the daemon's offer RPCs and report an empty set / an unknown offer honestly.
//! The populated path (a real offer from a paired device) is covered in-process by
//! `crates/txtodo-daemon/src/workspace_offer_grpc_tests.rs`; a two-daemon pairing run is
//! `default-workspace`'s own still-open test line.
// Integration tests are tests: clippy.toml allows unwrap/expect in test fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::process::Output;
use support::global_daemon::GlobalDaemon;

const WORKSPACE_ID: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";
const DEVICE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A daemon plus an empty workspace directory to run the CLI from.
fn fixture() -> (tempfile::TempDir, GlobalDaemon, tempfile::TempDir) {
    let state_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let daemon = GlobalDaemon::spawn(state_dir.path());
    let ws_dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    std::fs::write(ws_dir.path().join("todo.txt"), "").unwrap_or_else(|e| panic!("write: {e}"));
    (state_dir, daemon, ws_dir)
}

fn assert_fails_with(out: &Output, needle: &str) {
    assert!(!out.status.success(), "expected failure: {}", text(out));
    assert!(text(out).contains(needle), "{}", text(out));
}

#[test]
fn offers_lists_nothing_when_no_peer_offered_anything() {
    let (_state, daemon, ws) = fixture();
    let offers = daemon.txtodo(ws.path(), &["workspace", "offers"]);
    assert!(offers.status.success(), "{}", text(&offers));
    assert!(
        text(&offers).contains("no pending workspace offers"),
        "{}",
        text(&offers)
    );
    let offers_json = daemon.txtodo(ws.path(), &["--json", "workspace", "offers"]);
    assert!(offers_json.status.success(), "{}", text(&offers_json));
    assert!(
        text(&offers_json).trim().is_empty(),
        "{}",
        text(&offers_json)
    );
}

#[test]
fn accept_and_decline_of_an_unknown_offer_fail_by_name_and_adopt_nothing() {
    let (_state, daemon, ws) = fixture();
    let dir = ws.path();

    // No `--from`: the CLI looks the device up among the (empty) pending offers.
    let accept = daemon.txtodo(dir, &["workspace", "accept", WORKSPACE_ID, "--dir", "x"]);
    assert_fails_with(&accept, "no pending offer for workspace");

    // With `--from`: the daemon itself answers NotFound for a (device, workspace) it never saw.
    let target = dir.join("adopted");
    let target_arg = target.display().to_string();
    let accept_from = daemon.txtodo(
        dir,
        &[
            "workspace",
            "accept",
            WORKSPACE_ID,
            "--from",
            DEVICE_ID,
            "--dir",
            &target_arg,
        ],
    );
    assert_fails_with(&accept_from, "no pending offer");
    assert!(!target.exists(), "nothing adopted");

    let decline = daemon.txtodo(
        dir,
        &["workspace", "decline", WORKSPACE_ID, "--from", DEVICE_ID],
    );
    assert_fails_with(&decline, "no pending offer for workspace");

    // A non-ULID id is refused by the daemon's own parse, not silently matched.
    let bad = daemon.txtodo(
        dir,
        &["workspace", "decline", "not-a-ulid", "--from", DEVICE_ID],
    );
    assert_fails_with(&bad, "not a ULID");
}
