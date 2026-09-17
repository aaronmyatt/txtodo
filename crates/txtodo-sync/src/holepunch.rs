//! Connect/accept over a relay-configured endpoint (plan M8 `sync-relay-enable`, design §4.5).
//! `relay.rs` owns endpoint construction; this module owns dialing and accepting, mirroring
//! `lan_link.rs`'s split between `endpoint.rs` and itself. Hole-punching is not hand-rolled here:
//! iroh's own connection establishment already tries a direct path first and falls back to
//! relaying through the endpoint's configured relay when the punch fails or the peer is asleep
//! (design §4.5's "Hole-punching is iroh's default direct path; the relay is rendezvous +
//! fallback") — `connect` below is one `Endpoint::connect` call, same as `LanEndpoint::connect`.
//! Refs: <https://docs.rs/iroh> · <https://www.iroh.computer/docs/layers/relay>.

use std::fmt;

use iroh::endpoint::{ConnectError, ConnectingError, TransportAddrUsage};
use iroh::{EndpointAddr, RelayUrl};

use crate::lan_link::IrohLink;
use crate::message::GroupId;
use crate::relay::{RelayConfig, RelayError, build_endpoint};

/// Why a relay dial, accept or stream operation failed. Every variant names what was attempted
/// (CLAUDE.md §3), same split as `LanError`: a peer's own protocol violation is `Link::recv`'s job
/// to report via `LinkError`, not this type's.
#[derive(Debug)]
pub enum HolepunchError {
    /// `connect`'s caller asked for a peer in a group other than the one this endpoint was bound
    /// for. Refused before ever dialing — design §5's "gate rendezvous on `Hello.group`" applies
    /// to the relay path exactly as it does to LAN's `PeerTable::observe`.
    ForeignGroup {
        /// The group the caller asked to reach.
        requested: GroupId,
    },
    /// `node` was not a valid iroh public key. Validated, not asserted: these bytes come from a
    /// paired device record, external state this crate does not control the shape of.
    InvalidPeerId,
    /// `connect()` failed before a `Connection` existed.
    Connect(ConnectError),
    /// The connection closed, was refused, or otherwise never became usable.
    Connection(ConnectingError),
    /// No incoming connection ever arrived (the endpoint closed while accepting).
    NoIncoming,
    /// Opening or accepting the one bidirectional stream this crate's protocol runs over failed.
    Stream(String),
}

impl fmt::Display for HolepunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HolepunchError::ForeignGroup { requested } => {
                write!(
                    f,
                    "refusing to dial group {requested:?}: not this endpoint's group"
                )
            }
            HolepunchError::InvalidPeerId => write!(f, "node id is not a valid public key"),
            HolepunchError::Connect(e) => write!(f, "connect: {e}"),
            HolepunchError::Connection(e) => write!(f, "connection: {e}"),
            HolepunchError::NoIncoming => write!(f, "the endpoint stopped accepting connections"),
            HolepunchError::Stream(reason) => write!(f, "stream: {reason}"),
        }
    }
}

impl std::error::Error for HolepunchError {}

/// Which physical path a relay-configured connection is using right now — `relay-converge-test`
/// item 1's own ask ("assert convergence came via direct QUIC, not relay forwarding"), which
/// needed exactly this: iroh itself decides hole-punch vs. relay-forwarding internally and
/// exposes no single `ConnType` enum for it (unlike older `iroh` releases) — this is composed
/// from `Endpoint::remote_info`'s currently-active `TransportAddr` instead. A snapshot, not a
/// permanent property (`RemoteInfo`'s own doc: "may already be outdated by the time you are
/// reading this") — a punch that later degrades to relay, or vice versa, would read differently
/// on a second call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnPath {
    /// The active address is a direct IP path — the hole-punch succeeded.
    Direct,
    /// The active address is the relay — no direct path is in use right now.
    Relayed,
    /// `remote_info` returned nothing (endpoint closed, or the remote is unknown to it yet), or no
    /// address reports `Active` — genuinely undetermined, never guessed as one or the other.
    Unknown,
}

/// Composes [`ConnPath`] for `connection`'s remote, as `endpoint` currently sees it.
async fn conn_path(endpoint: &iroh::Endpoint, connection: &iroh::endpoint::Connection) -> ConnPath {
    let Some(info) = endpoint.remote_info(connection.remote_id()).await else {
        return ConnPath::Unknown;
    };
    match info
        .addrs()
        .find(|a| matches!(a.usage(), TransportAddrUsage::Active))
    {
        Some(a) if a.addr().is_relay() => ConnPath::Relayed,
        Some(a) if a.addr().is_ip() => ConnPath::Direct,
        _ => ConnPath::Unknown,
    }
}

/// Opt-in only, and deliberately its own env var rather than reusing `TXTODO_TEST_HOOKS`
/// (`crates/txtodo-daemon/src/debug_hooks.rs`): that flag is already set broadly across this
/// crate's own test harness (`support/mod.rs`, `support/multi.rs`, `support/relay.rs`) for an
/// unrelated purpose (`DebugSetGroupKey`), and every one of those callers would otherwise pay
/// [`POLL_WINDOW`]'s extra latency on every relay connect whether or not they care about this
/// question at all.
const CONN_PATH_POLL_ENV_VAR: &str = "TXTODO_CONN_PATH_POLL";

/// `relay-converge-test`'s own follow-up (id `01M2Q4RELAYPUNCHPOLL00001`): one immediate
/// [`conn_path`] snapshot can never observe iroh's hole-punch upgrading a connection from relay to
/// direct *after* `connect()` returns, since that upgrade (if any) happens asynchronously. Bounded
/// so a test that enables it never hangs the connect call.
const POLL_WINDOW: std::time::Duration = std::time::Duration::from_secs(3);
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(300);

/// One event per successful [`RelayEndpoint::connect`], naming which path it's using right now —
/// split out of `connect` purely to keep clippy's `cognitive_complexity` under this crate's budget
/// (the same reason `crates/txtodo-daemon/src/mutation.rs`'s `log_mutation_ops` is its own fn).
/// Never delays `connect`'s own return, with [`CONN_PATH_POLL_ENV_VAR`] set or not: `endpoint` and
/// `connection` are both cheap-clone handles to shared state (`iroh::Endpoint` wraps an `Arc`,
/// `iroh::endpoint::Connection`'s own doc: "may be cloned to obtain another handle to the same
/// connection"), so the poll below runs as its own spawned task the caller never awaits.
fn log_conn_path(endpoint: &iroh::Endpoint, connection: &iroh::endpoint::Connection) {
    let endpoint = endpoint.clone();
    let connection = connection.clone();
    tokio::spawn(async move {
        let path = conn_path(&endpoint, &connection).await;
        tracing::info!(path = ?path, "relay_connect_established");
        if std::env::var(CONN_PATH_POLL_ENV_VAR).as_deref() != Ok("1") {
            return;
        }
        let start = std::time::Instant::now();
        while start.elapsed() < POLL_WINDOW {
            tokio::time::sleep(POLL_INTERVAL).await;
            let polled = conn_path(&endpoint, &connection).await;
            tracing::info!(
                path = ?polled,
                elapsed_ms = start.elapsed().as_millis() as u64,
                "relay_connect_path_poll"
            );
        }
    });
}

/// A relay-configured endpoint, plus what a caller needs to dial or accept peers over it. One per
/// daemon, same lifetime shape as `LanEndpoint`.
pub struct RelayEndpoint {
    endpoint: iroh::Endpoint,
    relay_url: RelayUrl,
    group: GroupId,
}

impl RelayEndpoint {
    /// This endpoint's iroh identity, the bytes a peer needs to `connect` back to it — same shape
    /// as `LanEndpoint::node_id_bytes`.
    pub fn node_id_bytes(&self) -> [u8; 32] {
        *self.endpoint.id().as_bytes()
    }

    /// Resolves once this endpoint has actually registered with its relay. A peer dialing before
    /// that has nothing to reach yet — the relay only routes to an endpoint it has an open
    /// connection to (`accept`'s caller should await this before advertising itself as dialable).
    pub async fn online(&self) {
        self.endpoint.online().await;
    }

    /// Binds [`crate::relay::build_endpoint`] for `group` — every `connect` this endpoint makes is
    /// gated on the peer sharing this same group.
    pub async fn bind(cfg: &RelayConfig, group: GroupId) -> Result<RelayEndpoint, RelayError> {
        let relay_url: RelayUrl = cfg.url.parse().map_err(RelayError::InvalidUrl)?;
        let endpoint = build_endpoint(cfg).await?;
        Ok(RelayEndpoint {
            endpoint,
            relay_url,
            group,
        })
    }

    /// Same as [`RelayEndpoint::bind`], but binds with a caller-supplied identity seed instead of a
    /// fresh random one — task `daemon-workspace-identity-agreement` stage 1. The seed is opaque
    /// bytes here on purpose: the caller (`txtodo-daemon`'s `DeviceIdentity`) never names an `iroh`
    /// type, so this is the one crossing point where a persisted seed becomes a real relay identity.
    pub async fn bind_with_secret_key(
        cfg: &RelayConfig,
        group: GroupId,
        secret_key_bytes: [u8; 32],
    ) -> Result<RelayEndpoint, RelayError> {
        let relay_url: RelayUrl = cfg.url.parse().map_err(RelayError::InvalidUrl)?;
        let endpoint = crate::relay::build_endpoint_with_secret_key(cfg, secret_key_bytes).await?;
        Ok(RelayEndpoint {
            endpoint,
            relay_url,
            group,
        })
    }

    /// Test-only twin of [`RelayEndpoint::bind`] that skips relay TLS certificate verification, so
    /// a test can dial `iroh::test_utils::run_relay_server`'s self-signed local relay.
    /// `#[cfg(test)]` compiles this out of every real build entirely.
    #[cfg(test)]
    pub(crate) async fn bind_insecure_for_test(
        cfg: &RelayConfig,
        group: GroupId,
    ) -> Result<RelayEndpoint, RelayError> {
        let relay_url: RelayUrl = cfg.url.parse().map_err(RelayError::InvalidUrl)?;
        let endpoint = crate::relay::build_endpoint_insecure_for_test(cfg).await?;
        Ok(RelayEndpoint {
            endpoint,
            relay_url,
            group,
        })
    }

    /// Dials `node` — assumed reachable through this endpoint's own configured relay, the only one
    /// this device knows about (design §4.5 scopes M8 to one relay; a multi-relay directory is not
    /// this task). Refuses before dialing if `their_group` is not this endpoint's own group.
    pub async fn connect(
        &self,
        node: [u8; 32],
        their_group: GroupId,
    ) -> Result<IrohLink, HolepunchError> {
        if their_group != self.group {
            return Err(HolepunchError::ForeignGroup {
                requested: their_group,
            });
        }
        let id = iroh::PublicKey::from_bytes(&node).map_err(|_| HolepunchError::InvalidPeerId)?;
        let target = EndpointAddr::new(id).with_relay_url(self.relay_url.clone());
        let connection = self
            .endpoint
            .connect(target, crate::endpoint::ALPN)
            .await
            .map_err(HolepunchError::Connect)?;
        log_conn_path(&self.endpoint, &connection);
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|e| HolepunchError::Stream(e.to_string()))?;
        Ok(IrohLink::new(connection, send, recv))
    }

    /// Dials `node` for a pairing connection — the relay twin of `LanEndpoint::connect_pairing`
    /// (`lan_link.rs`), and this endpoint's own [`RelayEndpoint::connect`] minus its `GroupId`
    /// gate: pairing has no shared group yet (the whole point of pairing), so nothing here can
    /// check one. The actual security boundary is downstream, unchanged by this method: the
    /// daemon's `pairing_lan.rs::process_hello` refuses any `JoinerHello` whose `nonce`/`group`
    /// do not match its own active offer — carrier-agnostic already, so this dial needs no gate
    /// of its own (`tasks/sync-pairing-relay/notes.md`'s design decision, option (a)). Uses
    /// [`crate::endpoint::PAIRING_ALPN`] instead of [`crate::endpoint::ALPN`] so a pairing
    /// connection is never mistaken for the group-keyed sync protocol, same ALPN-based
    /// separation `endpoint.rs`'s module doc already established for LAN.
    pub async fn connect_pairing(&self, node: [u8; 32]) -> Result<IrohLink, HolepunchError> {
        let id = iroh::PublicKey::from_bytes(&node).map_err(|_| HolepunchError::InvalidPeerId)?;
        let target = EndpointAddr::new(id).with_relay_url(self.relay_url.clone());
        let connection = self
            .endpoint
            .connect(target, crate::endpoint::PAIRING_ALPN)
            .await
            .map_err(HolepunchError::Connect)?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|e| HolepunchError::Stream(e.to_string()))?;
        Ok(IrohLink::new(connection, send, recv))
    }

    /// Dials `node` for the always-on control channel (task
    /// `daemon-workspace-identity-agreement` stage 5) — the third sibling of
    /// [`RelayEndpoint::connect`]/[`RelayEndpoint::connect_pairing`], same shape as
    /// `connect_pairing`: no `GroupId` gate of its own. Unlike pairing, a control connection *is*
    /// between two already-paired devices sharing a group key — but the gate that matters here is
    /// "does this device even want to redial `node`", decided by the caller against its own known-
    /// peers roster (`IdentityStore::list_devices()`'s `relay_node_id` column) before ever calling
    /// this, not by the dial itself. Uses [`crate::endpoint::CONTROL_ALPN`] so a control connection
    /// is never mistaken for the group-keyed sync protocol or a pairing handshake.
    pub async fn connect_control(&self, node: [u8; 32]) -> Result<IrohLink, HolepunchError> {
        let id = iroh::PublicKey::from_bytes(&node).map_err(|_| HolepunchError::InvalidPeerId)?;
        let target = EndpointAddr::new(id).with_relay_url(self.relay_url.clone());
        let connection = self
            .endpoint
            .connect(target, crate::endpoint::CONTROL_ALPN)
            .await
            .map_err(HolepunchError::Connect)?;
        let (send, recv) = connection
            .open_bi()
            .await
            .map_err(|e| HolepunchError::Stream(e.to_string()))?;
        Ok(IrohLink::new(connection, send, recv))
    }

    /// Waits for one incoming connection and wraps it as a [`crate::link::Link`]. Cannot gate on
    /// group before accepting — the accepting side has no way to know who is dialing until the
    /// connection exists; `Session::on_hello`'s own group check is what actually protects it once
    /// frames start flowing.
    pub async fn accept(&self) -> Result<IrohLink, HolepunchError> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or(HolepunchError::NoIncoming)?;
        let connection = incoming.await.map_err(HolepunchError::Connection)?;
        let (send, recv) = connection
            .accept_bi()
            .await
            .map_err(|e| HolepunchError::Stream(e.to_string()))?;
        Ok(IrohLink::new(connection, send, recv))
    }
}
