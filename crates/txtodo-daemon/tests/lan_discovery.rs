//! Step 3 of the `sync-lan-transport` daemon-wiring brief: two REAL `txtodod` processes, same
//! sync group, finding each other over real mDNS on this machine's real LAN interface.
//!
//! **Scope.** This test proves discovery specifically, at the full daemon level: `lan::start`
//! binds a real `iroh` endpoint, `Discovery::start` advertises under `_txtodo._udp` with this
//! device's real `TXT_NODE`, and the *other* real process's `Discovery::browse` resolves it — the
//! `lan_peer_found` log line is the externally observable proof (`lan_peers.rs`'s doc comment on
//! that log line). It stops there rather than also asserting a synced outcome —
//! `tests/lan_loopback_converge.rs` is the fuller proof that a real connect and a real sync round
//! trip both work between two real processes on this host (this task's step 3 initially suspected
//! a same-*host* connect bug; it turned out to be same-*process*-only — see that test's module
//! doc and `crates/txtodo-sync/src/endpoint_tests.rs` for the full, corrected diagnosis).

#![allow(clippy::expect_used, clippy::unwrap_used)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;

/// Generous: mDNS resolution was ~0.8s in `txtodo-sync`'s own real-discovery test, but two full
/// daemon processes (walk, store, watcher, endpoint bind all happen first) need more headroom.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Polls `daemon`'s JSON log for any `lan_peer_found` line. The sync group is randomised per test
/// run (below), so any sighting at all can only be this test's own peer — a developer's own
/// daemon on this LAN, or another concurrently-running test run, advertises a different group and
/// is filtered out inside `PeerTable::observe` before it would ever be logged.
fn wait_for_a_peer_to_be_found(daemon: &Daemon, deadline: Instant) {
    loop {
        let log = daemon.log_tail();
        if log.lines().any(|l| l.contains("lan_peer_found")) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "never saw lan_peer_found within {DISCOVERY_TIMEOUT:?}\n--- log ---\n{log}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// A tiny, dependency-free source of test-run entropy for the group id — this only needs to be
/// different across runs, not cryptographically random (real group id/key generation lives in
/// `txtodo-sync`/`workspace.rs`; discovery itself never touches the group *key*, only its id, so
/// this test — proving discovery, not sync — never needs one).
fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

#[tokio::test]
async fn two_real_daemons_discover_each_other_over_real_mdns() {
    // Same, chosen group id, seeded before either process starts — see
    // `Daemon::start_with_seeded_group`'s doc for why `DebugSetGroupKey` (a live RPC, reachable
    // only after `lan.rs` already registered its mDNS advertisement) cannot do this instead.
    let group_id = rand_u128();
    let mut a = Daemon::start_with_seeded_group("buy milk\n", "sidecar", group_id).await;
    let mut b = Daemon::start_with_seeded_group("walk the dog\n", "sidecar", group_id).await;

    let health_a = a.health().await;
    let health_b = b.health().await;
    assert!(health_a.lan_relay_disabled);
    assert!(health_b.lan_relay_disabled);

    let deadline = Instant::now() + DISCOVERY_TIMEOUT;
    wait_for_a_peer_to_be_found(&a, deadline);
    wait_for_a_peer_to_be_found(&b, deadline);
}
