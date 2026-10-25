//! Task `peer-prune`, ADR 0044: a device that goes away is pruned by its peer, and pairing again
//! brings it back with what it missed. Two REAL `txtodod` processes pair, one is frozen past the
//! peer's quiet threshold (`TXTODO_PRUNE_QUIET_AFTER`, 4 s here, a week by default), the peer drops it
//! and rotates the group key, both sides edit while apart, and a second pairing makes them one
//! group again: each file ends with every line.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

use crate::support;

use std::time::{Duration, Instant};
use support::Daemon;

const A_FIRST: &str = "(A) buy milk id:01M2D3AAAAAAAAAAAAAAAAAAAA";
const A_WHILE_APART: &str = "write to the bank id:01M2D3BBBBBBBBBBBBBBBBBBBB";
const B_WHILE_APART: &str = "call the plumber id:01M2D3CCCCCCCCCCCCCCCCCCCC";

/// The quiet threshold, and the time the prune may take on top of it: the session to a frozen peer
/// closes after 20 s without a heartbeat, then the threshold runs from there.
const PRUNE_DEADLINE: Duration = Duration::from_secs(90);
/// Long enough for several resync rounds (1 s each here) to have crossed anything that could.
const APART_FOR: Duration = Duration::from_secs(6);
const CONVERGE_DEADLINE: Duration = Duration::from_secs(60);

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

fn lines(bytes: &[u8]) -> Vec<String> {
    let mut lines: Vec<String> = String::from_utf8_lossy(bytes)
        .lines()
        .map(str::to_string)
        .collect();
    lines.sort();
    lines
}

/// Polls until both daemons hold `want` (sorted lines), or fails with both logs.
async fn wait_for_lines(a: &mut Daemon, b: &mut Daemon, want: &[&str]) {
    let mut want: Vec<String> = want.iter().map(|s| (*s).to_string()).collect();
    want.sort();
    let start = Instant::now();
    loop {
        let (got_a, got_b) = (
            lines(&a.daemon_bytes().await),
            lines(&b.daemon_bytes().await),
        );
        if got_a == want && got_b == want {
            return;
        }
        assert!(
            start.elapsed() < CONVERGE_DEADLINE,
            "no convergence in {CONVERGE_DEADLINE:?}\nwant={want:?}\na={got_a:?}\nb={got_b:?}\n--- a ---\n{}\n--- b ---\n{}",
            a.log_tail(),
            b.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Whether `a`'s device table says its peer is removed.
async fn peer_removed(a: &mut Daemon) -> bool {
    a.device_list()
        .await
        .iter()
        .any(|d| !d.is_self && d.removed)
}

#[tokio::test]
async fn a_pruned_device_pairs_again_and_catches_up() {
    let workspace_id = rand_u128();
    let mut a = Daemon::start_with_workspace_id_and_envs(
        &format!("{A_FIRST}\n"),
        workspace_id,
        &[("TXTODO_PRUNE_QUIET_AFTER", "4s")],
    )
    .await;
    let mut b = Daemon::start_with_workspace_id("", workspace_id).await;
    a.pair_with(&mut b).await;
    wait_for_lines(&mut a, &mut b, &[A_FIRST]).await;

    // B goes away without a word.
    b.pause();
    let start = Instant::now();
    while !peer_removed(&mut a).await {
        assert!(
            start.elapsed() < PRUNE_DEADLINE,
            "A did not prune the frozen B in {PRUNE_DEADLINE:?}\n--- a ---\n{}",
            a.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let after_prune = a.device_list().await;
    assert!(
        after_prune.iter().any(|d| !d.is_self && d.removed),
        "{after_prune:?}"
    );

    // Both sides keep working while apart.
    a.external_write(&format!("{A_FIRST}\n{A_WHILE_APART}\n"));
    b.resume();
    b.external_write(&format!("{A_FIRST}\n{B_WHILE_APART}\n"));
    a.settle().await;
    b.settle().await;

    // B is dropped: nothing crosses while it stays that way (A refuses its Hello, and what A seals
    // under the new key B cannot open).
    tokio::time::sleep(APART_FOR).await;
    let (got_a, got_b) = (
        lines(&a.daemon_bytes().await),
        lines(&b.daemon_bytes().await),
    );
    assert!(
        !got_a.iter().any(|l| l == B_WHILE_APART),
        "A got B's line: {got_a:?}"
    );
    assert!(
        !got_b.iter().any(|l| l == A_WHILE_APART),
        "B got A's line: {got_b:?}"
    );

    a.pair_with(&mut b).await;
    wait_for_lines(&mut a, &mut b, &[A_FIRST, A_WHILE_APART, B_WHILE_APART]).await;
    let devices = a.device_list().await;
    let peer = devices.iter().find(|d| !d.is_self).expect("B is A's peer");
    assert!(
        !peer.removed,
        "pairing again brought the row back: {peer:?}"
    );
}
