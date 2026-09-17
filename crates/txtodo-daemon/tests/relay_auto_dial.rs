//! Root todo `sync-pairing-relay-ongoing-dial`: two real `txtodod` processes, `--no-lan` on both,
//! pair over the relay (`sync-pairing-relay`) with no `--relay-dial-peer` set on either side, and
//! prove ongoing sync then converges a real file edit anyway — the gap this task found and fixed
//! (`relay_autodial.rs`). Same honestly-scoped sandbox as `relay_converge.rs`/`pairing_relay.rs`
//! (macOS, no root, one host, real n0 public relay) — see those files' own module docs for why.
//!
//! **Why this reuses `pairing_relay.rs`'s real pairing instead of a debug seam.** Auto-dial reads
//! `record_peer_relay_reachability`, which today is only ever populated by a real pairing
//! handshake completing (`pairing_lan.rs::finish_joiner`) — there is no test-only seam that seeds
//! it directly (adding one would touch `txtodo-proto`, a separate crate/slice from this one).
//! Driving a real handshake is also the more faithful proof: it exercises the exact path a real
//! user hits, not a synthetic shortcut.
//!
//! **One-directional recording, so only one side can auto-dial.** `finish_joiner` records only
//! the *initiator's* relay reachability on the *joiner* (`relay_autodial.rs`'s own module doc) —
//! the reverse direction is a documented, separate gap. So after pairing, only the joiner (`b`
//! below) has anything to auto-dial with; this test proves that direction, which is exactly the
//! direction `sync-pairing-relay-ongoing-dial`'s bug report described (a freshly relay-paired
//! device never dialing back for ongoing sync).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;
use support::relay::start_with_seeded_group_args;

const PAIR_DEADLINE: Duration = Duration::from_secs(110);
const RELAY_BIND_DEADLINE: Duration = Duration::from_secs(20);
const AUTO_DIAL_DEADLINE: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// One of iroh's own documented public relay servers — the same one `relay_converge.rs` and
/// `pairing_relay.rs` use.
const RELAY_URL: &str = "https://use1-1.relay.n0.iroh.link";

/// Serializes this file's real-relay test against `pairing_relay.rs`/`relay_converge.rs`'s own
/// serialization — a second independent mutex would still let this file's test race those files'
/// real-daemon processes for the same external relay's connection handling. A `tokio::sync::Mutex`
/// scoped to just this file is the best this crate can do without a cross-file lock; if this proves
/// flaky alongside its siblings under `cargo test --workspace`, the fix is the same one those files
/// already flag (a `--test-threads=1` real-relay test group), not a change here.
static SERIALIZE_REAL_RELAY_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

fn response_to_code(r: &txtodo_proto::v1::PairOfferResponse) -> String {
    serde_json::json!({
        "device": r.device,
        "group_id": r.group_id,
        "x25519_pub": r.x25519_pub,
        "endpoint": r.endpoint,
        "nonce": r.nonce,
        "identity_mode": r.identity_mode,
        "relay_node_id": r.relay_node_id,
        "relay_url": r.relay_url,
    })
    .to_string()
}

async fn wait_for_relay_bound(daemon: &mut Daemon, label: &str) {
    let start = Instant::now();
    loop {
        let outcome = daemon.health().await.relay_last_outcome;
        if outcome.starts_with("bound as ") {
            return;
        }
        assert!(
            start.elapsed() < RELAY_BIND_DEADLINE,
            "{label}: relay endpoint never bound within {RELAY_BIND_DEADLINE:?} (last outcome: {outcome:?})"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn await_peer_sas(a: &mut Daemon) -> String {
    let start = Instant::now();
    loop {
        let sas = a.pair_await_peer().await.sas;
        if !sas.is_empty() {
            return sas;
        }
        assert!(
            start.elapsed() < PAIR_DEADLINE,
            "no joiner reached the initiator within {PAIR_DEADLINE:?}\n--- a's log ---\n{}",
            a.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_group_key(b: &mut Daemon) {
    let start = Instant::now();
    loop {
        if b.health().await.lan_group_key_present {
            return;
        }
        assert!(
            start.elapsed() < PAIR_DEADLINE,
            "the group key never landed on the joiner within {PAIR_DEADLINE:?}\n--- b's log ---\n{}",
            b.log_tail()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Polls `to`'s bytes against `from`'s until they match or [`AUTO_DIAL_DEADLINE`] passes — same
/// shape as `relay_converge.rs::wait_for_relay_convergence`, duplicated rather than shared since
/// that helper is private to its own file.
async fn wait_for_convergence(from: &mut Daemon, to: &mut Daemon, label: &str) {
    let want = from.daemon_bytes().await;
    let start = Instant::now();
    loop {
        let got = to.daemon_bytes().await;
        if got == want {
            eprintln!(
                "relay-auto-dial[{label}]: converged in {:?}",
                start.elapsed()
            );
            return;
        }
        assert!(
            start.elapsed() < AUTO_DIAL_DEADLINE,
            "{label}: did not converge within {AUTO_DIAL_DEADLINE:?} — auto-dial never kicked in?\nwant={:?}\ngot={:?}\n--- {label} peer log ---\n{}",
            String::from_utf8_lossy(&want),
            String::from_utf8_lossy(&got),
            to.log_tail(),
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// The bug this task fixes: pair two real `txtodod` processes over the relay alone (no shared
/// LAN), then edit `a`'s file — with no `--relay-dial-peer` on either side, `b` (the joiner, the
/// only side holding `a`'s recorded relay reachability) must auto-dial `a` on its own periodic
/// resync tick and pick up the edit, purely from `relay_autodial.rs`'s new path.
#[tokio::test]
#[ignore = "real-network variance against n0's public relay, same class this suite's sibling real-pairing tests (pairing_relay.rs) already accept and quarantine — a real pairing handshake (not just an already-established session, unlike relay_converge.rs's cheaper debug-seeded path) is needed here since auto-dial reads the devices table only a real pairing populates"]
async fn joiner_auto_dials_initiator_via_relay_after_pairing_with_no_dial_peer_flag() {
    let _serialize = SERIALIZE_REAL_RELAY_TESTS.lock().await;

    let mut a = start_with_seeded_group_args(
        &[("todo.txt", "(A) buy milk id:01M2N016AUTODIALTESTA001\n")],
        "tagged",
        rand_u128(),
        &["--relay".into(), RELAY_URL.into(), "--no-lan".into()],
    )
    .await;
    wait_for_relay_bound(&mut a, "a").await;

    let mut b = start_with_seeded_group_args(
        &[("todo.txt", "")],
        "tagged",
        rand_u128(),
        &["--relay".into(), RELAY_URL.into(), "--no-lan".into()],
    )
    .await;
    wait_for_relay_bound(&mut b, "b").await;

    let offer = a.pair_offer().await;
    let code = response_to_code(&offer);
    b.pair_accept(code).await;
    await_peer_sas(&mut a).await;
    a.pair_confirm_sas().await;
    b.pair_confirm_sas().await;
    wait_for_group_key(&mut b).await;

    // No `--relay-dial-peer` on either side, anywhere above: the only thing that can make `b`
    // reach `a` for the initial snapshot (and any further edit) is `relay_autodial.rs`'s own
    // periodic resync-tick check against the devices table `finish_joiner` just populated.
    wait_for_convergence(&mut a, &mut b, "initial-snapshot").await;

    // A later edit on `a` must also converge — proves this is `lan.rs`'s own periodic redial
    // shape (auto-dial runs every resync tick, not just once right after pairing), not a
    // one-shot dial that happens to catch the initial snapshot and nothing after.
    a.external_write(
        "(A) buy milk id:01M2N016AUTODIALTESTA001\n(B) walk the dog id:01M2N016AUTODIALTESTB001\n",
    );
    a.settle().await;
    wait_for_convergence(&mut a, &mut b, "later-edit").await;
}
