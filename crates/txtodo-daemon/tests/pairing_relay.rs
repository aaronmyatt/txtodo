//! Plan M8 `sync-pairing-relay`: pairing itself (not just ongoing sync, `relay-converge-test`'s
//! job) falls back to a relay-carried handshake when LAN can't reach the peer. Same honestly-
//! scoped sandbox technique as `relay_converge.rs` (its own module doc explains why: macOS, no
//! root, no real network-namespace boundary) — two real `txtodod` processes, `--no-lan` on both,
//! against one of iroh's own public relay servers.
//!
//! **Scope.** These tests prove pairing completes over the relay carrier alone (the group key
//! lands on the joiner) — they do not also prove *ongoing* sync then converges a file over relay
//! for a device pair with no shared LAN and no `--relay-dial-peer`: that needs a persisted, shared
//! identity across carriers, the same known gap `relay_fallback.rs`'s own doc names, and is
//! `relay-converge-test`'s (already-merged) territory, not duplicated here.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stderr)]
#![cfg(unix)]

mod support;

use std::time::{Duration, Instant};
use support::Daemon;
use support::relay::start_with_seeded_group_args;
use txtodo_proto::v1 as pb;
use txtodo_sync::{
    DEVICE_STATIC_KEY_BYTES, GroupId, InitiatorReply, JoinerHello, Link, RelayConfig,
    RelayEndpoint, X25519_PUBLIC_KEY_BYTES,
};

/// Bounded just under the daemon's own `PAIRING_WINDOW_MS` (120 s) — not `relay_converge.rs`'s
/// much shorter 30 s, which measures convergence over an *already-established* long-lived session
/// (its own `--relay-dial-peer` retries every second from well before that test's timer starts).
/// Pairing's one-shot relay bursts are each a fresh connection (`pairing_relay_dial.rs::
/// relay_attempt`, deliberately unbounded per `LAN_RACE_TIMEOUT`'s own doc), and this sandbox's
/// path to `RELAY_URL` measured directly, repeatedly, as unusually unreliable per burst — one run
/// needed ~24 failed one-shot connections before the 25th succeeded (~50 s) — real, measured, and
/// outside this suite's control, the same class of caveat `relay_converge.rs`'s own module doc
/// already gives this exact relay dependency. The daemon's own production retry loop already
/// tolerates this (it simply keeps
/// retrying for the full `PAIRING_WINDOW_MS`); this test's deadline reflects that real window
/// rather than asserting a tighter number production itself does not guarantee.
const PAIR_DEADLINE: Duration = Duration::from_secs(110);
/// How long a daemon gets to bind its relay endpoint (real TLS handshake to a real relay).
const RELAY_BIND_DEADLINE: Duration = Duration::from_secs(20);
/// One of iroh's own documented public relay servers — the same one `relay_converge.rs` uses.
const RELAY_URL: &str = "https://use1-1.relay.n0.iroh.link";

/// Serializes this file's two real-daemon tests: each spins up two real `txtodod` processes
/// dialing the same real, external relay, and Rust's default parallel test runner would otherwise
/// run both at once — four processes competing for this sandbox's CPU and the one public relay's
/// connection handling was enough to occasionally push a pairing retry round past
/// [`PAIR_DEADLINE`] (observed directly: each test is reliable alone, only flaky run together).
/// A `tokio::sync::Mutex` rather than `std::sync::Mutex` because the guard is held across many
/// `.await` points for a whole test's duration.
static SERIALIZE_REAL_RELAY_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn rand_u128() -> u128 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    (u128::from(hasher.finish()) << 64) | u128::from(hasher.finish().wrapping_add(1))
}

/// Field-for-field the same shape `pairing_wire.rs::response_to_code` builds — this integration
/// test is a separate binary and cannot reach that `pub(crate)` function directly.
fn response_to_code(r: &pb::PairOfferResponse) -> String {
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

/// Polls `daemon.health().relay_last_outcome` until `relay.rs::bind` has actually bound, so a test
/// never dials before the endpoint is registered with the relay.
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

/// Two real `txtodod` processes, `--no-lan` on both, complete `pair_offer`/`pair_accept`/
/// `pair_confirm_sas` with no shared LAN path at all — only a real relay carries the handshake.
/// Proves notes.md's acceptance bullet: "the same SAS-confirmation UX as the LAN path", and that
/// the offer itself now carries a non-empty relay rendezvous once the initiator's relay endpoint
/// is bound (todo item 2's own no-regression twin is `pairing_grpc_tests::
/// qr_payload_has_no_field_beyond_the_documented_eight`, asserting the *empty* case).
#[tokio::test]
async fn two_real_daemons_pair_over_relay_with_lan_disabled() {
    let _serialize = SERIALIZE_REAL_RELAY_TESTS.lock().await;
    let mut a = start_with_seeded_group_args(
        &[("todo.txt", "(A) buy milk id:01M2CZ00000000000000000A\n")],
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
    assert!(
        !offer.relay_node_id.is_empty(),
        "a's offer must carry its own relay rendezvous once bound"
    );
    assert!(!offer.relay_url.is_empty());
    let code = response_to_code(&offer);

    let sas_b = b.pair_accept(code).await.sas;
    assert!(
        !sas_b.is_empty(),
        "the joiner computes its SAS locally, no network needed"
    );
    let sas_a = await_peer_sas(&mut a).await;
    assert_eq!(
        sas_a, sas_b,
        "both real devices derive the identical SAS, carried over the relay alone"
    );

    a.pair_confirm_sas().await;
    b.pair_confirm_sas().await;

    wait_for_group_key(&mut b).await;

    // txtodo doctor's carrier report (todo item 5): both sides recorded "relay", not "lan" — there
    // was no LAN path available at all for either to have used instead.
    assert_eq!(a.health().await.pairing_last_carrier, "relay");
    assert_eq!(b.health().await.pairing_last_carrier, "relay");
}

/// A dummy hello over the real relay, built from `offer`'s own device/group but a caller-supplied
/// `nonce` — the shape a relay client without the real QR/code would have to guess. `#[allow(
/// clippy::too_many_arguments)]`-free: bundled fields the caller already has, so this stays under
/// the 5-argument cap without a second struct.
fn bogus_hello(offer: &pb::PairOfferResponse, nonce: [u8; 16]) -> JoinerHello {
    JoinerHello {
        device: txtodo_model::DeviceId::new(txtodo_model::Ulid::from_u128(rand_u128())),
        group: GroupId(offer.group_id.parse().expect("offer group_id is decimal")),
        nonce,
        public_key: [0u8; X25519_PUBLIC_KEY_BYTES],
        static_public: [0u8; DEVICE_STATIC_KEY_BYTES],
        confirmed: false,
    }
}

/// Sends `hello` to `node` over `endpoint` and reads one reply — the test's own minimal stand-in
/// for `pairing_lan.rs::attempt`'s LAN twin, driving the identical wire protocol directly rather
/// than through a second full daemon (this is the *attacker's* dial, which by definition never
/// goes through the real `txtodo pair <code>` path). Takes an already-bound `&RelayEndpoint`
/// rather than binding its own — binding a fresh endpoint (and its own cold relay registration)
/// per attempt was itself the dominant source of "no reply" flakiness measured in this test before
/// this fix, not the security property under test.
async fn dial_and_send(
    endpoint: &RelayEndpoint,
    node: [u8; 32],
    hello: JoinerHello,
) -> Option<InitiatorReply> {
    let mut link = endpoint.connect_pairing(node).await.ok()?;
    tokio::task::spawn_blocking(move || {
        let frame = hello.encode().ok()?;
        link.send(frame).ok()?;
        let reply_frame = link.recv().ok()?;
        InitiatorReply::decode(&reply_frame).ok()
    })
    .await
    .ok()
    .flatten()
}

/// Retries [`dial_and_send`] over the same already-bound `endpoint` until it gets *any* reply or
/// [`PAIR_DEADLINE`] passes — a `None` (connect/send/recv failed) is a transient real-network
/// hiccup, not itself the security assertion; `hello` is rebuilt per attempt since `JoinerHello` is
/// consumed by value.
async fn retry_dial_and_send(
    endpoint: &RelayEndpoint,
    node: [u8; 32],
    hello: impl Fn() -> JoinerHello,
) -> Option<InitiatorReply> {
    let start = Instant::now();
    loop {
        if let Some(reply) = dial_and_send(endpoint, node, hello()).await {
            return Some(reply);
        }
        if start.elapsed() >= PAIR_DEADLINE {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// todo item 8 (security): a relay client that knows the initiator's relay node id (public,
/// learned the same way any joiner would) but *not* the offer's real nonce cannot complete a
/// pairing — `process_hello`'s existing nonce/group check (unchanged by this task, notes.md's
/// design decision option (a)) refuses it, over the relay exactly as it already did over LAN.
/// Then proves the attack left the real offer usable: a real joiner with the real code still
/// pairs normally right after.
#[tokio::test]
async fn a_relay_dial_with_the_wrong_nonce_cannot_complete_a_pairing() {
    let _serialize = SERIALIZE_REAL_RELAY_TESTS.lock().await;
    let mut a = start_with_seeded_group_args(
        &[("todo.txt", "(A) buy milk id:01M2CZ00000000000000000B\n")],
        "tagged",
        rand_u128(),
        &["--relay".into(), RELAY_URL.into(), "--no-lan".into()],
    )
    .await;
    wait_for_relay_bound(&mut a, "a").await;

    let offer = a.pair_offer().await;
    let node: [u8; 32] = hex_decode32(&offer.relay_node_id);

    // The attacker's own relay endpoint, bound once and warmed up (`online().await`) before any
    // dial attempt — bound fresh per retry was itself the dominant source of flakiness measured
    // here (a cold endpoint's own relay registration competing with the dial timing), not the
    // security property under test.
    let attacker_cfg = RelayConfig {
        url: RELAY_URL.to_string(),
        max_peers: 1,
    };
    let attacker = RelayEndpoint::bind(&attacker_cfg, GroupId(0))
        .await
        .expect("attacker bind");
    attacker.online().await;

    // A guess, not the real nonce hex-decoded from `offer.nonce` — the attacker's whole point is
    // not knowing it. `dial_and_send` is a single connection attempt over a real public relay
    // (same real-network variability `pairing_relay_dial.rs::relay_attempt`'s own retry loop
    // absorbs in production); retry the *dial* itself here rather than treating a transient
    // connect hiccup as a false pass of the security property under test.
    let wrong_nonce = [0x99u8; 16];
    let reply = retry_dial_and_send(&attacker, node, || bogus_hello(&offer, wrong_nonce)).await;
    assert!(
        matches!(reply, Some(InitiatorReply::Rejected)),
        "a wrong nonce must be rejected outright, not left pending or granted: {reply:?}\n--- a's log ---\n{}",
        a.log_tail()
    );
    // The bogus attempt must not have consumed or corrupted the real pairing attempt: a's own
    // active offer is still exactly what pair_offer returned, still awaiting the real joiner.
    assert!(
        a.pair_await_peer().await.sas.is_empty(),
        "the bogus hello must not itself count as a real joiner arriving"
    );

    // The real joiner, with the real code, still pairs normally right after.
    let mut b = start_with_seeded_group_args(
        &[("todo.txt", "")],
        "tagged",
        rand_u128(),
        &["--relay".into(), RELAY_URL.into(), "--no-lan".into()],
    )
    .await;
    wait_for_relay_bound(&mut b, "b").await;
    let code = response_to_code(&offer);
    let sas_b = b.pair_accept(code).await.sas;
    let sas_a = await_peer_sas(&mut a).await;
    assert_eq!(
        sas_a, sas_b,
        "the real pairing still succeeds after the attack"
    );
}

/// Lowercase hex to exactly 32 bytes — `offer.relay_node_id` is this test's own trusted input
/// (produced by `pairing_grpc.rs::response_of` in this same process tree), so a decode failure is
/// a real bug in the test/production wire format, worth an honest panic rather than a silent skip.
fn hex_decode32(s: &str) -> [u8; 32] {
    assert_eq!(s.len(), 64, "relay_node_id must be 64 hex chars: {s:?}");
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("valid hex");
    }
    out
}
