//! The real `iroh`-backed [`Link`] (plan M4 `sync-lan-transport`, daemon-wiring pass). `endpoint.rs`
//! owns the one endpoint constructor; this module owns everything that *uses* the bound endpoint —
//! dialing, accepting, and turning a QUIC connection's one bidirectional stream into a `Link`.
//! `iroh` also appears in `relay.rs`/`holepunch.rs` (plan M8 `sync-relay-enable`), which reuses
//! [`IrohLink`] rather than duplicating it — a `Link` over one QUIC connection's stream is the same
//! type regardless of whether the connection reached its peer via LAN or relay (check
//! `.claude/budgets.json`'s `allowedDeps` before wiring `iroh` in anywhere else). `txtodo-daemon`
//! sees only [`LanEndpoint`], [`IrohLink`] and [`LanError`] from this module; it never names an
//! `iroh` type directly.
//!
//! **Known upstream blocker, confirmed both here and in `endpoint_tests.rs`:** `noq-proto` 1.3.0
//! (vendored by `iroh` 1.2.0) refuses a same-host connection — the connecting side's source IP
//! numerically equal to the accepting side's own bound IP, reported as plain IPv4 on one side and
//! IPv4-mapped-IPv6 on the other. [`LanEndpoint::connect`] filters out loopback candidate addresses
//! (see its doc) as basic hygiene, but that does **not** avoid this bug: it also fires for a real
//! non-loopback LAN address when both endpoints happen to be the same machine, which is exactly
//! this sandbox's situation (see `endpoint_tests.rs`'s
//! `two_real_bind_local_endpoints_on_the_same_host_hit_the_same_bug`, `#[ignore]`d with the full
//! trace evidence). Two genuinely distinct hosts on a real LAN are not expected to hit it. This
//! module's job is to be *ready* for that real LAN the day it is available, not to work around a
//! bug in a dependency it does not own.
//!
//! Refs: <https://docs.rs/iroh> · <https://www.iroh.computer/docs>.

use std::fmt;
use std::net::SocketAddr;

use iroh::endpoint::{BindError, ConnectError, ConnectingError, WriteError};
use iroh::{Endpoint, EndpointAddr};

use crate::endpoint::{ALPN, PAIRING_ALPN, bind_local_endpoint};
use crate::frame::{Frame, FrameError};
use crate::link::{Link, LinkError};

/// Why a LAN dial, accept or stream operation failed. Every variant names what was attempted
/// (CLAUDE.md §3); a peer's own protocol violation is `Link::recv`'s job to report via
/// [`LinkError`], not this type's.
#[derive(Debug)]
pub enum LanError {
    /// The local endpoint could not bind.
    Bind(BindError),
    /// `connect()` failed before a `Connection` existed.
    Connect(ConnectError),
    /// The connection closed, was refused, or otherwise never became usable.
    Connection(ConnectingError),
    /// No incoming connection ever arrived (the endpoint closed while accepting).
    NoIncoming,
    /// Opening or accepting the one bidirectional stream this crate's protocol runs over failed.
    Stream(String),
    /// A peer advertised no address at all worth dialing (every candidate was loopback or empty).
    NoDialableAddress,
}

impl fmt::Display for LanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LanError::Bind(e) => write!(f, "bind the LAN endpoint: {e}"),
            LanError::Connect(e) => write!(f, "connect: {e}"),
            LanError::Connection(e) => write!(f, "connection: {e}"),
            LanError::NoIncoming => write!(f, "the endpoint stopped accepting connections"),
            LanError::Stream(reason) => write!(f, "stream: {reason}"),
            LanError::NoDialableAddress => {
                write!(f, "the peer advertised no non-loopback address to dial")
            }
        }
    }
}

impl std::error::Error for LanError {}

/// The bound local endpoint, plus everything a caller needs to advertise itself over `Discovery`
/// and dial or accept peers. One per daemon.
pub struct LanEndpoint {
    endpoint: Endpoint,
}

impl LanEndpoint {
    /// Binds [`bind_local_endpoint`] (relay off, port mapping off, every local interface).
    pub async fn bind() -> Result<LanEndpoint, LanError> {
        let endpoint = bind_local_endpoint().await.map_err(LanError::Bind)?;
        Ok(LanEndpoint { endpoint })
    }

    /// This endpoint's iroh identity, opaque bytes for `Discovery::start`'s `TXT_NODE` field
    /// (`discovery.rs` never names `iroh` itself, so the conversion lives here).
    pub fn node_id_bytes(&self) -> [u8; 32] {
        *self.endpoint.id().as_bytes()
    }

    /// One port to advertise over mDNS: the first bound socket's, preferring IPv4 since that is
    /// what most LANs route without extra configuration. `None` only if the endpoint has no bound
    /// socket at all, which `bind()` never actually produces.
    pub fn advertise_port(&self) -> Option<u16> {
        let sockets = self.endpoint.bound_sockets();
        sockets
            .iter()
            .find(|a| a.is_ipv4())
            .or_else(|| sockets.first())
            .map(SocketAddr::port)
    }

    /// Dials `node` at one of `addrs`, opens the one bidirectional stream this crate's protocol
    /// runs over, and wraps it as a [`Link`]. Loopback candidates are deprioritised, not refused:
    /// a real multi-host LAN never advertises one in the first place, but `enable_addr_auto()`
    /// occasionally resolves *only* a loopback address for a same-host peer before its real
    /// interface address is known (observed in `txtodo-daemon`'s real two-process discovery test)
    /// — refusing that outright would make LAN sync flaky in exactly the same-host situation this
    /// module already documents extensively. Real non-loopback addresses are preferred whenever
    /// any are present; loopback is used only when it is all there is.
    pub async fn connect(
        &self,
        node: [u8; 32],
        addrs: &[SocketAddr],
    ) -> Result<IrohLink, LanError> {
        self.connect_with_alpn(node, addrs, ALPN).await
    }

    /// [`LanEndpoint::connect`], but negotiates [`PAIRING_ALPN`] instead of [`ALPN`] — plan M4
    /// `sync-pairing`'s LAN wiring pass. The accepting side (`txtodo-daemon`'s `lan.rs`) tells the
    /// two connection kinds apart by [`IrohLink::alpn`], never by frame content: a pairing
    /// connection has no group key to seal anything with in the first place.
    pub async fn connect_pairing(
        &self,
        node: [u8; 32],
        addrs: &[SocketAddr],
    ) -> Result<IrohLink, LanError> {
        self.connect_with_alpn(node, addrs, PAIRING_ALPN).await
    }

    /// [`LanEndpoint::connect`]'s control-channel twin: a workspace-offer exchange with a paired
    /// peer found on the LAN, no relay needed.
    pub async fn connect_control(
        &self,
        node: [u8; 32],
        addrs: &[SocketAddr],
    ) -> Result<IrohLink, LanError> {
        self.connect_with_alpn(node, addrs, crate::endpoint::CONTROL_ALPN)
            .await
    }

    async fn connect_with_alpn(
        &self,
        node: [u8; 32],
        addrs: &[SocketAddr],
        alpn: &[u8],
    ) -> Result<IrohLink, LanError> {
        let has_real_address = addrs.iter().any(|a| !a.ip().is_loopback());
        let dialable: Vec<SocketAddr> = addrs
            .iter()
            .copied()
            .filter(|a| !has_real_address || !a.ip().is_loopback())
            .collect();
        if dialable.is_empty() {
            return Err(LanError::NoDialableAddress);
        }
        let id = iroh::PublicKey::from_bytes(&node).map_err(|_| LanError::NoDialableAddress)?;
        let mut target = EndpointAddr::new(id);
        for addr in dialable {
            target = target.with_ip_addr(addr);
        }
        let connection = self
            .endpoint
            .connect(target, alpn)
            .await
            .map_err(LanError::Connect)?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|e| LanError::Stream(e.to_string()))?;
        Ok(IrohLink::new(connection, send, recv))
    }

    /// Waits for one incoming connection, accepts its one bidirectional stream, and wraps it as a
    /// [`Link`]. Bounded by the caller's own timeout/select — this never loops internally. The
    /// caller reads [`IrohLink::alpn`] to tell a sync connection from a pairing one (both are
    /// accepted here, since [`bind_local_endpoint`](crate::bind_local_endpoint) registers both
    /// ALPNs on the same endpoint).
    pub async fn accept(&self) -> Result<IrohLink, LanError> {
        let incoming = self.endpoint.accept().await.ok_or(LanError::NoIncoming)?;
        let connection = incoming.await.map_err(LanError::Connection)?;
        let (send, recv) = connection
            .accept_bi()
            .await
            .map_err(|e| LanError::Stream(e.to_string()))?;
        Ok(IrohLink::new(connection, send, recv))
    }
}

/// A real [`Link`] over one iroh QUIC connection's one bidirectional stream. `send`/`recv` are
/// synchronous (the `Link` trait's own shape — see `link.rs`'s module doc on why: `Session` stays
/// testable without an executor), so this struct blocks a dedicated driver thread on the tokio
/// `Handle` it was built with; nothing here may run on a tokio worker thread directly, or blocking
/// would starve the runtime's other tasks. `recv` reports [`LinkError::Closed`] after
/// [`IDLE_TIMEOUT`] of silence, not just on a real close — see its doc for why a caller should
/// expect (and periodically redial through) short-lived sessions rather than one connection
/// staying open for a whole pairing's lifetime.
pub struct IrohLink {
    /// Kept alive so the stream's in-flight data is not torn down early (same reasoning as
    /// `endpoint_tests.rs`'s comment on why both connection handles must outlive the exchange).
    _connection: iroh::endpoint::Connection,
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
    handle: tokio::runtime::Handle,
    /// Bytes read from the stream but not yet decoded into a whole `Frame`.
    inbox: Vec<u8>,
    /// The ALPN this connection negotiated (`ALPN` or `PAIRING_ALPN`), captured once at
    /// construction so a caller accepting on one shared endpoint can route the connection without
    /// ever naming an `iroh` type itself (see [`IrohLink::alpn`]).
    alpn: Vec<u8>,
    /// How long one `recv` waits for bytes before reporting `Closed` — chosen by ALPN at
    /// construction ([`IDLE_TIMEOUT`] or [`PAIRING_IDLE_TIMEOUT`]).
    idle_timeout: std::time::Duration,
}

/// Largest chunk read from the stream at once; bounds `inbox`'s growth between frame boundaries
/// the same way every other wire buffer in this crate is capped.
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// How long `recv` waits for new bytes before treating the link as idle and reporting
/// [`LinkError::Closed`] — the same outcome as a real close, so a caller driving a session over
/// this link ends it the same way either way. This bounds a session's lifetime once both sides go
/// quiet, so a periodic redial can open a fresh connection and pick up state that changed locally
/// after this one went idle, instead of one connection holding its slot forever.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(750);

/// [`IDLE_TIMEOUT`]'s pairing-ALPN counterpart. A pairing round is one `JoinerHello` → one
/// `InitiatorReply`, and the reply may cross a public relay both ways plus the initiator's keystore
/// read before it arrives; 750 ms was too tight for that from Asia against n0's relay (2026-09-22).
/// Also bounds [`Link::finish`]'s wait for the peer's ack — the other half of the same incident:
/// the initiator dropped its link right after `send`, and the QUIC close discarded the reply.
/// Sync sessions keep the short value: their redial cadence depends on it (see [`IDLE_TIMEOUT`]).
const PAIRING_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl IrohLink {
    /// `pub(crate)` rather than private: `holepunch.rs` (plan M8 `sync-relay-enable`) wraps a
    /// relay-dialed connection the identical way — a `Link` over one QUIC connection's stream
    /// doesn't care whether the connection reached its peer via LAN or relay.
    pub(crate) fn new(
        connection: iroh::endpoint::Connection,
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
    ) -> IrohLink {
        let alpn = connection.alpn().to_vec();
        let idle_timeout = if alpn == crate::endpoint::PAIRING_ALPN {
            PAIRING_IDLE_TIMEOUT
        } else {
            IDLE_TIMEOUT
        };
        IrohLink {
            _connection: connection,
            send,
            recv,
            // Panics only if called outside a tokio runtime, which every caller of
            // `LanEndpoint::connect`/`accept` (themselves `async fn`s) satisfies by construction.
            handle: tokio::runtime::Handle::current(),
            inbox: Vec::new(),
            alpn,
            idle_timeout,
        }
    }

    /// Which protocol this connection negotiated: [`crate::endpoint::ALPN`] for a group-keyed sync
    /// session, [`crate::endpoint::PAIRING_ALPN`] for a pairing relay connection. `txtodo-daemon`'s
    /// `lan.rs` accept loop reads this to route an incoming connection to the right driver.
    pub fn alpn(&self) -> &[u8] {
        &self.alpn
    }
}

impl Link for IrohLink {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
        let bytes = frame.encode().map_err(LinkError::Frame)?;
        self.handle
            .block_on(self.send.write_all(&bytes))
            .map_err(|e: WriteError| LinkError::Io(e.to_string()))
    }

    /// Thin span wrapper around `recv_inner` (`#[instrument]` on the real loop overflows
    /// `cognitive_complexity`, `tasks/logging-sync-crate/notes.md`).
    #[tracing::instrument(skip_all)]
    fn recv(&mut self) -> Result<Frame, LinkError> {
        self.recv_inner(self.idle_timeout)
            .and_then(|frame| frame.ok_or_else(|| log_link_idle_timeout(self.idle_timeout)))
    }

    /// Quinn's `read` is cancel-safe, so a timed-out wait loses no bytes: whatever arrived stays
    /// in the stream, and a partial frame stays in `inbox`.
    /// Ref: <https://docs.rs/quinn/latest/quinn/struct.RecvStream.html#method.read>
    fn recv_timeout(&mut self, wait: std::time::Duration) -> Result<Option<Frame>, LinkError> {
        self.recv_inner(wait)
    }

    /// `finish()` sends the stream's FIN; `stopped()` then resolves once the peer has acknowledged
    /// every byte before it (or stopped the stream) — the only signal that makes dropping the
    /// connection right afterwards safe. Bounded by the same idle timeout as `recv`.
    /// Ref: <https://docs.rs/quinn/latest/quinn/struct.SendStream.html#method.stopped>
    fn finish(&mut self) -> Result<(), LinkError> {
        self.send
            .finish()
            .map_err(|e| LinkError::Io(e.to_string()))?;
        let acked = tokio::time::timeout(self.idle_timeout, self.send.stopped());
        match self.handle.block_on(acked) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(LinkError::Io(e.to_string())),
            Err(_elapsed) => Err(log_link_idle_timeout(self.idle_timeout)),
        }
    }
}

impl IrohLink {
    /// The disambiguation the backlog line names: both terminal `Closed` sites below stay the same
    /// `LinkError::Closed` at the type level (no wire/API break — see `tasks/logging-sync-crate/
    /// notes.md`), but each now logs which one it was *before* returning, so a human reading the
    /// JSON log can finally tell an idle redial apart from a real peer hangup.
    /// `Ok(None)` once `wait` passes with no whole frame; `recv` turns that into the idle close.
    fn recv_inner(&mut self, wait: std::time::Duration) -> Result<Option<Frame>, LinkError> {
        loop {
            match Frame::decode(&self.inbox) {
                Ok((frame, used)) => {
                    self.inbox.drain(..used);
                    return Ok(Some(frame));
                }
                Err(FrameError::Truncated { .. }) => {} // fall through: read more
                Err(other) => return Err(LinkError::Frame(other)),
            }
            let mut chunk = [0u8; READ_CHUNK_BYTES];
            let read = tokio::time::timeout(wait, self.recv.read(&mut chunk));
            match self.handle.block_on(read) {
                Ok(Ok(Some(n))) => self.inbox.extend_from_slice(&chunk[..n]),
                Ok(Ok(None)) => return Err(log_link_peer_closed()),
                Ok(Err(e)) => return Err(LinkError::Io(e.to_string())),
                Err(_elapsed) => return Ok(None),
            }
        }
    }
}

/// A real peer closed the stream (`recv` read `None`, i.e. EOF) — distinct from
/// `log_link_idle_timeout` below even though both return the identical `LinkError::Closed`.
fn log_link_peer_closed() -> LinkError {
    tracing::debug!("link_peer_closed");
    LinkError::Closed
}

/// `IDLE_TIMEOUT` elapsed with no bytes at all — by design (module doc), not a real close; a
/// caller is expected to redial. Distinct from `log_link_peer_closed` above.
fn log_link_idle_timeout(idle_timeout: std::time::Duration) -> LinkError {
    tracing::debug!(millis = idle_timeout.as_millis(), "link_idle_timeout");
    LinkError::Closed
}
