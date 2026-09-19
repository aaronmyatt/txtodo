//! Integration test (tasks/tui): `Daemon::sync_status` round-trips through a real `txtodod` over
//! the real ADR 0010 unix socket — the TUI-side half of the SyncStatus chain (proto + daemon
//! handler were proven in their own crates' tests; this is the one thing only this crate can
//! prove, that its own thin client actually calls the real RPC and gets a real, well-formed
//! response back).
//!
//! `#[ignore]`d (2026-09-19): spawns a real daemon; CI-only, see `tests/daemon_autostart.rs`'s
//! own doc comment for the full rationale shared across this crate's real-daemon tests.
#![cfg(unix)]

mod support;

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn sync_status_round_trips_against_a_real_daemon_with_no_peers() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;

    let resp = daemon
        .sync_status()
        .await
        .unwrap_or_else(|e| panic!("sync_status: {e}"));

    assert!(
        resp.peers.is_empty(),
        "a freshly seeded workspace has no paired peers"
    );
    assert_eq!(resp.pending_ops, 0, "nothing to be pending against");
}
