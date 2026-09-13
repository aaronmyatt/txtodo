//! Plan M4 acceptance (`sync-pairing`'s LAN wiring pass): two REAL `txtodod` processes complete a
//! real cross-device pairing handshake over the real LAN transport — `PairOffer` -> `PairAccept`
//! -> `PairConfirmSas` on both sides, driven entirely through gRPC, never the `DebugSetGroupKey`
//! test-only seam `sync-loopback-converge`'s test uses. Asserts both sides derive the identical
//! SAS words, the group key actually lands on the joiner (`Health.lan_group_key_present`), and the
//! joiner ends up with the initiator's real file — not by any dedicated snapshot RPC, but because
//! adopting a shared group id/key is all pairing does; `lan.rs`'s own already-proven sync engine
//! (unchanged by this task) discovers the now-matching peer and replays its ops from genesis, the
//! same mechanism `nested_ref_sync.rs` proved for a fresh device.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;
use txtodo_proto::v1 as pb;

/// Generous: real mDNS discovery, the pairing relay's own retry burst, and (once paired) the
/// group-changed mDNS re-advertisement `lan.rs` now performs, can each take real wall-clock time
/// on a shared CI runner (`crates/txtodo-cli/tests/pairing.rs`'s own end-to-end test uses the same
/// order of magnitude).
const PAIR_DEADLINE: Duration = Duration::from_secs(30);

/// The JSON `code` `PairAccept` parses — field-for-field `PairOfferResponse`'s own six fields, the
/// same shape `pairing_wire.rs::response_to_code` builds (`pub(crate)`, so this integration test,
/// a separate binary, cannot reuse it directly and mirrors it instead).
fn response_to_code(r: &pb::PairOfferResponse) -> String {
    serde_json::json!({
        "device": r.device,
        "group_id": r.group_id,
        "x25519_pub": r.x25519_pub,
        "endpoint": r.endpoint,
        "nonce": r.nonce,
        "identity_mode": r.identity_mode,
    })
    .to_string()
}

/// Polls `PairAwaitPeer` (initiator only) until a real joiner's handshake reaches this device.
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

/// Polls `Health.lan_group_key_present` until the joiner's background relay task
/// (`pairing_lan.rs`) has adopted the real group key the initiator sealed and sent.
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

/// Polls `b`'s file against `a`'s until they match — the real acceptance bar: the joiner actually
/// received the initiator's file over the LAN, not merely its own (empty) starting content.
async fn wait_for_file_convergence(a: &mut Daemon, b: &mut Daemon) {
    let want = a.daemon_bytes().await;
    let start = Instant::now();
    loop {
        let got = b.daemon_bytes().await;
        if got == want {
            return;
        }
        assert!(
            start.elapsed() < PAIR_DEADLINE,
            "the joiner's file did not converge to the initiator's within {PAIR_DEADLINE:?}\nwant={:?}\ngot={:?}",
            String::from_utf8_lossy(&want),
            String::from_utf8_lossy(&got),
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn two_real_daemons_pair_for_real_and_the_joiner_receives_the_initiators_file() {
    let mut a = Daemon::start("(A) buy milk id:01M2D3AAAAAAAAAAAAAAAAAAAA\n").await;
    let mut b = Daemon::start("").await;

    let offer = a.pair_offer().await;
    assert_eq!(offer.identity_mode, "tagged");
    let code = response_to_code(&offer);

    let sas_b = b.pair_accept(code).await.sas;
    assert!(
        !sas_b.is_empty(),
        "the joiner computes its SAS locally, no network needed"
    );

    let sas_a = await_peer_sas(&mut a).await;
    assert_eq!(
        sas_a, sas_b,
        "both real devices derive the identical SAS from the same handshake"
    );

    // Both sides confirm — order does not matter, `pairing_lan.rs`'s relay carries whichever
    // arrives first to the other side on its own retry cadence.
    a.pair_confirm_sas().await;
    b.pair_confirm_sas().await;

    wait_for_group_key(&mut b).await;
    wait_for_file_convergence(&mut a, &mut b).await;
}
