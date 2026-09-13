//! The real `iroh`-backed [`Link`] (plan M4 `sync-lan-transport`, daemon-wiring pass). `endpoint.rs`
//! owns the one endpoint constructor; this module owns everything that *uses* the bound endpoint —
//! dialing, accepting, and turning a QUIC connection's one bidirectional stream into a `Link` — so
//! it is still true that `iroh` appears in exactly these two files and nowhere else (check
//! `.claude/budgets.json`'s `allowedDeps` before wiring anything else to it). `txtodo-daemon` sees
//! only [`LanEndpoint`], [`IrohLink`] and [`LanError`] from this module; it never names an `iroh`
//! type directly.
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

use crate::endpoint::{ALPN, bind_local_endpoint};
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
    /// runs over, and wraps it as a [`Link`]. Loopback candidates are dropped before dialing: they
    /// are never useful on a real LAN and are exactly the shape of address that the module doc's
    /// upstream bug was first found on, so refusing them here is cheap defence in depth even though
    /// it does not fully avoid the bug (see the module doc).
    pub async fn connect(
        &self,
        node: [u8; 32],
        addrs: &[SocketAddr],
    ) -> Result<IrohLink, LanError> {
        let dialable: Vec<SocketAddr> = addrs
            .iter()
            .copied()
            .filter(|a| !a.ip().is_loopback())
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
            .connect(target, ALPN)
            .await
            .map_err(LanError::Connect)?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|e| LanError::Stream(e.to_string()))?;
        Ok(IrohLink::new(connection, send, recv))
    }

    /// Waits for one incoming connection, accepts its one bidirectional stream, and wraps it as a
    /// [`Link`]. Bounded by the caller's own timeout/select — this never loops internally.
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
/// would starve the runtime's other tasks.
pub struct IrohLink {
    /// Kept alive so the stream's in-flight data is not torn down early (same reasoning as
    /// `endpoint_tests.rs`'s comment on why both connection handles must outlive the exchange).
    _connection: iroh::endpoint::Connection,
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
    handle: tokio::runtime::Handle,
    /// Bytes read from the stream but not yet decoded into a whole `Frame`.
    inbox: Vec<u8>,
}

/// Largest chunk read from the stream at once; bounds `inbox`'s growth between frame boundaries
/// the same way every other wire buffer in this crate is capped.
const READ_CHUNK_BYTES: usize = 64 * 1024;

impl IrohLink {
    fn new(
        connection: iroh::endpoint::Connection,
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
    ) -> IrohLink {
        IrohLink {
            _connection: connection,
            send,
            recv,
            // Panics only if called outside a tokio runtime, which every caller of
            // `LanEndpoint::connect`/`accept` (themselves `async fn`s) satisfies by construction.
            handle: tokio::runtime::Handle::current(),
            inbox: Vec::new(),
        }
    }
}

impl Link for IrohLink {
    fn send(&mut self, frame: Frame) -> Result<(), LinkError> {
        let bytes = frame.encode().map_err(LinkError::Frame)?;
        self.handle
            .block_on(self.send.write_all(&bytes))
            .map_err(|e: WriteError| LinkError::Io(e.to_string()))
    }

    fn recv(&mut self) -> Result<Frame, LinkError> {
        loop {
            match Frame::decode(&self.inbox) {
                Ok((frame, used)) => {
                    self.inbox.drain(..used);
                    return Ok(frame);
                }
                Err(FrameError::Truncated { .. }) => {} // fall through: read more
                Err(other) => return Err(LinkError::Frame(other)),
            }
            let mut chunk = [0u8; READ_CHUNK_BYTES];
            match self.handle.block_on(self.recv.read(&mut chunk)) {
                Ok(Some(n)) => self.inbox.extend_from_slice(&chunk[..n]),
                Ok(None) => return Err(LinkError::Closed),
                Err(e) => return Err(LinkError::Io(e.to_string())),
            }
        }
    }
}
