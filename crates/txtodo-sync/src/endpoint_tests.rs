//! The relay-empty proof — "the single most useful test in this task" per the task notes, because
//! it is the one that fails the day an iroh upgrade changes what a preset defaults to — and a real
//! loopback exchange of one `Frame` over an actual bound QUIC connection.

use std::time::Duration;

use iroh::Watcher;
use iroh::endpoint::presets::Minimal;
use iroh::{Endpoint, RelayMode};

use crate::endpoint::{ALPN, bind_local_endpoint};
use crate::frame::Frame;

/// Bounded: a hung connect/accept fails the test rather than the CI job.
const TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// [`bind_local_endpoint`], but bound only to `127.0.0.1`/`::1` instead of every interface — the
/// hermetic, interface-independent way a test would prove the connect/accept/stream path works.
/// Kept only for [`the_configured_relay_set_is_empty`]; see the `#[ignore]` reason on
/// `two_loopback_endpoints_exchange_one_frame` below for why the connect path itself can't be
/// exercised this way right now.
async fn bind_loopback_endpoint() -> Endpoint {
    Endpoint::builder(Minimal)
        .relay_mode(RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .bind()
        .await
        .unwrap()
}

#[tokio::test]
async fn the_configured_relay_set_is_empty() {
    let endpoint = bind_local_endpoint().await.unwrap();
    let mut watcher = endpoint.home_relay_status();
    let relays = watcher.get();
    assert!(
        relays.is_empty(),
        "relay must be off by construction, not by omission: {relays:?}"
    );
}

/// `addr()` is a snapshot; immediately after `bind()` it may carry no direct address yet — direct
/// address discovery runs in the background. `Endpoint::online()` is the wrong wait here (it means
/// "reachable via a relay", which `pend`s forever with relay disabled, by that method's own docs);
/// waiting on `watch_addr()` for a direct IP instead is what LAN-only connectivity needs.
async fn wait_for_direct_address(endpoint: &Endpoint) -> iroh::EndpointAddr {
    let mut watcher = endpoint.watch_addr();
    loop {
        let addr = watcher.get();
        if addr.ip_addrs().next().is_some() {
            return addr;
        }
        watcher
            .updated()
            .await
            .expect("the endpoint outlives this wait");
    }
}

/// Confirmed on both macOS and Linux (Docker `rust:1.95-bookworm`): `noq_proto::endpoint::refuse`
/// fires with `network_path=(local: 127.0.0.1, remote: [::ffff:127.0.0.1]:_)` — an upstream
/// address-family mismatch in `noq-proto` 1.3.0 (vendored by `iroh` 1.2.0, latest as of 2026-09-12)
/// when both ends bind literally to `127.0.0.1`, not a sandbox artifact. Source:
/// `noq-proto-1.3.0/src/endpoint.rs:738`, the `refuse()` the trace names. Raw UDP loopback (tiny and
/// 1200-byte, both directions) was verified to work fine on this same path, ruling out a socket or
/// MTU issue below iroh. Left `#[ignore]` rather than deleted or worked around.
///
/// **2026-09-13 correction (`sync-lan-transport`, wiring pass): this is not a 127.0.0.1-specific
/// bug.** The note above (and this crate's own `CLAUDE.md`) previously claimed `bind_local_endpoint`
/// — the real, production, bind-all-interfaces constructor — was "not known to hit this path". That
/// was wrong: [`two_real_bind_local_endpoints_in_the_same_process_hit_the_same_bug`] below reproduces
/// the identical `refusing incoming` trace line using `bind_local_endpoint()` unmodified, dialed via
/// this machine's real LAN address (`192.168.100.24` in the environment it was found in), not
/// `127.0.0.1` at all. Also ruled out as the cause at the time: `PortmapperConfig` being left at its
/// default `Enabled` (disabled in `endpoint.rs` regardless, as its own stray-default problem), and
/// dual-stack (`[::]`) binding — an IPv4-only bind (`clear_ip_transports().bind_addr("0.0.0.0:0")`)
/// hit the identical refusal too.
///
/// **2026-09-13 second correction, same session (`sync-lan-transport` daemon-wiring pass): the
/// trigger is same-*process*, not same-host.** The paragraph above concluded "true for any
/// same-host connection" — that overreached. `txtodo-daemon`'s `tests/lan_loopback_converge.rs`
/// spawns two real, separate `txtodod` *processes* on this same host, dialing each other via the
/// same real LAN address family this test uses, and they connect and sync for real, repeatedly,
/// with no `refusing incoming` anywhere in either process's log. The two tests in this file both
/// construct **both** `Endpoint`s inside the same process (necessarily, being single-process unit
/// tests) — that is the actual precondition: two `iroh::Endpoint`s coexisting in one process,
/// something about their shared process-local state (global/thread-local caches, or how the OS
/// reports a path back to two sockets owned by the same process — not narrowed further, since the
/// real, correct answer for this crate is "test across processes instead," not chasing a one-off
/// upstream bug deeper than needed to route around it). A real LAN, with two distinct hosts *or*
/// two distinct processes on one host, is not expected to hit this at all. Re-enable both tests
/// (as same-process regression checks) once `noq-proto`/`iroh` ships a fix; there is no need to
/// wait for one to build real LAN sync, which is already proven working cross-process.
#[tokio::test]
#[ignore = "upstream noq-proto 1.3.0 refuses 127.0.0.1 <-> [::ffff:127.0.0.1] as a path mismatch; see doc comment"]
async fn two_loopback_endpoints_exchange_one_frame() {
    let a = bind_loopback_endpoint().await;
    let b = bind_loopback_endpoint().await;
    let b_addr = tokio::time::timeout(TEST_TIMEOUT, wait_for_direct_address(&b))
        .await
        .expect("b never discovered a direct address to be dialed on");

    let sent = Frame {
        version: 1,
        body: b"hello over a real quic connection".to_vec(),
    };
    let sent_bytes = sent.encode().unwrap();

    // Establish both ends of the connection first, and keep both handles alive for the whole
    // exchange: dropping a `Connection` can tear down in-flight, unacked stream data before the
    // peer finishes reading it, which is exactly the race a connection-per-closure would invite.
    let (conn_a, incoming) = tokio::time::timeout(TEST_TIMEOUT, async {
        tokio::join!(a.connect(b_addr, ALPN), b.accept())
    })
    .await
    .expect("connect/accept did not hang, but did not finish either");
    let conn_a = conn_a.unwrap();
    let conn_b = incoming
        .expect("an incoming connection arrives")
        .await
        .unwrap();

    let mut send_stream = conn_a.open_uni().await.unwrap();
    send_stream.write_all(&sent_bytes).await.unwrap();
    send_stream.finish().unwrap();

    let received = tokio::time::timeout(TEST_TIMEOUT, async {
        let mut recv_stream = conn_b.accept_uni().await.unwrap();
        recv_stream.read_to_end(sent_bytes.len() * 2).await.unwrap()
    })
    .await
    .expect("the peer did not receive the frame in time");

    let (frame, used) = Frame::decode(&received).unwrap();
    assert_eq!(used, received.len());
    assert_eq!(frame, sent);
}

/// The `sync-lan-transport` daemon-wiring pass's step-3 finding, reproduced with the real,
/// unmodified [`bind_local_endpoint`] — no loopback-forcing helper, no literal `127.0.0.1`
/// anywhere. Both endpoints bind to every interface exactly as production does; `b`'s own
/// `watch_addr()` direct address (this machine's real LAN IP) is what `a` dials. See the second
/// correction on [`two_loopback_endpoints_exchange_one_frame`] above for the full, final
/// diagnosis: **this test's own precondition — both `Endpoint`s living in this one process — is
/// what triggers the bug, not the same-host address it dials.** `txtodo-daemon`'s
/// `tests/lan_loopback_converge.rs` runs the equivalent connect between two real, separate
/// `txtodod` *processes* on this same host and it works, repeatedly, for real. Re-enable this test
/// (as a same-process regression check) once `noq-proto`/`iroh` ships a fix; real LAN sync does
/// not need to wait for that.
#[tokio::test]
#[ignore = "upstream noq-proto 1.3.0 refuses two Endpoints in the same process dialing each other; see doc comment — real LAN sync works fine across processes (txtodo-daemon's tests/lan_loopback_converge.rs)"]
async fn two_real_bind_local_endpoints_in_the_same_process_hit_the_same_bug() {
    let a = bind_local_endpoint().await.unwrap();
    let b = bind_local_endpoint().await.unwrap();
    let b_addr = tokio::time::timeout(TEST_TIMEOUT, wait_for_direct_address(&b))
        .await
        .expect("b never discovered a direct address to be dialed on");

    let (conn_a, incoming) = tokio::time::timeout(TEST_TIMEOUT, async {
        tokio::join!(a.connect(b_addr, ALPN), b.accept())
    })
    .await
    .expect("connect/accept did not hang, but did not finish either");
    let conn_a = conn_a.expect("a connects to b over a real, non-loopback-forced address");
    let _conn_b = incoming
        .expect("an incoming connection arrives")
        .await
        .expect("b accepts a's connection");
    drop(conn_a);
}
