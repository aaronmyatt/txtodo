//! Plan M4 acceptance (`sync-loopback-converge`): two REAL `txtodod` processes, paired through the
//! test-only seam (`DebugSetGroupKey` — real pairing has no transport over the LAN link yet), find
//! each other over real mDNS and converge a real external edit through the real LAN sync engine.
//!
//! **Pairing seam and its ordering.** The sync group id is seeded into each workspace's `oplog.db`
//! before either process starts (`Daemon::start_with_seeded_group` — see its doc: `lan.rs`
//! registers the mDNS advertisement once, at startup, off whatever `Workspace::group()` says right
//! then, so the id has to be right from the first advertisement). The group *key* is set via
//! `DebugSetGroupKey` immediately after each daemon reports ready, before this test does anything
//! else — `drive_session` checks for a key on every connection attempt (`lan_session.rs`'s
//! `fetch_group_key`), so a connection that races ahead of this call simply finds no key and does
//! nothing. That is not the reliability net, though: `lan.rs`'s periodic resync (every
//! `RESYNC_INTERVAL`, independent of mDNS re-announcement timing) is what guarantees another
//! attempt follows shortly regardless, which is also what lets the *second* edit below (made well
//! after the first session has gone idle) converge without this test depending on exactly when
//! mDNS happens to re-announce.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;

/// Plan M4's own number: "converge within 2 s (measured in test)", covering discovery, handshake
/// and one op round trip once both daemons are already paired and ready. This machine's mDNS
/// discovery measured ~0.8 s standalone (`txtodo-sync`'s own real-discovery test); this deadline
/// is generous on top of that for two full daemon processes and is a bounded poll, never a sleep.
const CONVERGE_DEADLINE: Duration = Duration::from_secs(2);

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

/// Pairs two already-started, same-group daemons through the test-only seam, immediately.
async fn pair(a: &mut Daemon, b: &mut Daemon, group_id: u128) {
    let key_hex = "ab".repeat(32);
    a.debug_set_group_key(&group_id.to_string(), &key_hex).await;
    b.debug_set_group_key(&group_id.to_string(), &key_hex).await;
}

/// Polls `to`'s bytes against `from`'s until they match, printing the measured time on success —
/// plan M4's own "record the measured time... so drift is visible" acceptance line. Fails at
/// `CONVERGE_DEADLINE` rather than hanging; never a bare sleep-then-assert.
async fn wait_for_convergence(from: &mut Daemon, to: &mut Daemon, label: &str) {
    let want = from.daemon_bytes().await;
    let start = Instant::now();
    loop {
        let got = to.daemon_bytes().await;
        if got == want {
            eprintln!(
                "sync-loopback-converge[{label}]: converged in {:?}",
                start.elapsed()
            );
            return;
        }
        assert!(
            start.elapsed() < CONVERGE_DEADLINE,
            "{label}: did not converge within {CONVERGE_DEADLINE:?}\nwant={:?}\ngot={:?}\n--- to's log ---\n{}",
            String::from_utf8_lossy(&want),
            String::from_utf8_lossy(&got),
            to.log_tail(),
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn two_real_daemons_converge_an_external_edit_in_both_directions() {
    let group_id = rand_u128();
    let mut a = Daemon::start_with_seeded_group(
        "buy milk id:01M2CZ00000000000000000A\n",
        "tagged",
        group_id,
    )
    .await;
    let mut b = Daemon::start_with_seeded_group("", "tagged", group_id).await;
    pair(&mut a, &mut b, group_id).await;

    // A -> B: an external write to A's file, through its own watcher/reconciler (a real Op, not a
    // synthetic one), must reach B.
    a.external_write(
        "buy milk id:01M2CZ00000000000000000A\nwalk the dog id:01M2CZ00000000000000000B\n",
    );
    a.settle().await;
    wait_for_convergence(&mut a, &mut b, "a-to-b").await;

    // B -> A: the same edit shape, reversed, over what should still be the same session (or a
    // fresh one — either is fine, this test only asserts the outcome).
    b.external_write("buy milk id:01M2CZ00000000000000000A\nwalk the dog id:01M2CZ00000000000000000B\nfeed the cat id:01M2CZ00000000000000000C\n");
    b.settle().await;
    wait_for_convergence(&mut b, &mut a, "b-to-a").await;

    assert_eq!(a.daemon_bytes().await, b.daemon_bytes().await);
}
