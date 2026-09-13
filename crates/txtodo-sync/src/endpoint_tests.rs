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
/// was wrong: [`two_real_bind_local_endpoints_on_the_same_host_hit_the_same_bug`] below reproduces
/// the identical `refusing incoming` trace line using `bind_local_endpoint()` unmodified, dialed via
/// this machine's real LAN address (`192.168.100.24` in the environment it was found in), not
/// `127.0.0.1` at all. The actual trigger is **the connecting side's source IP being numerically
/// equal to the accepting endpoint's own bound IP** — true for any same-host connection, loopback or
/// real-interface — which `noq-proto` represents as plain IPv4 on one side of the comparison and
/// IPv4-mapped-IPv6 on the other, then refuses as a path mismatch. Also ruled out as the cause:
/// `PortmapperConfig` being left at its default `Enabled` (disabled in `endpoint.rs` regardless, as
/// its own stray-default problem), and dual-stack (`[::]`) binding — an IPv4-only bind
/// (`clear_ip_transports().bind_addr("0.0.0.0:0")`) hits the identical refusal. A real LAN with two
/// distinct hosts (genuinely different source/destination IPs) is not expected to trigger this path
/// at all; only same-host testing is blocked. Re-enable both tests once `noq-proto`/`iroh` ships a
/// fix, or once a real two-machine LAN run is available.
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
/// `watch_addr()` direct address (this machine's real LAN IP) is what `a` dials. See the doc
/// comment on [`two_loopback_endpoints_exchange_one_frame`] above for the full diagnosis: the
/// upstream bug fires whenever the connecting side's source IP numerically equals the accepting
/// endpoint's own bound IP (any same-host connection), not specifically `127.0.0.1`. This is the
/// reason `sync-loopback-converge`/`sync-bench-m4`/`test-nested-ref-sync`'s two-real-daemon
/// convergence tests could not be built against a real `iroh` connection in this sandbox (a single
/// host, so both daemons necessarily share one machine's addresses) — only against two genuinely
/// distinct hosts. Re-enable once `noq-proto`/`iroh` ships a fix, or a real two-machine LAN is
/// available.
#[tokio::test]
#[ignore = "same upstream noq-proto 1.3.0 same-host refusal, reproduced with the real bind_local_endpoint() and a real LAN address, not a loopback-forcing helper; see doc comment"]
async fn two_real_bind_local_endpoints_on_the_same_host_hit_the_same_bug() {
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
