//! The relay-on twin of `endpoint.rs` (plan M8 `sync-relay-enable`, design §4.5). ADR 0024 dropped
//! LAN transport entirely: sync always goes through a configured relay now, there is no off-mode.
//! `endpoint.rs`'s constructor stays as the historical/test-only LAN-disabled shape; every real
//! caller binds through here instead. Same discipline as that module: one constructor, nothing
//! bypasses it, and the "configured relay set is non-empty" fact is asserted by a test so an iroh
//! upgrade that changes a default fails here, not silently.
//! Refs: <https://docs.rs/iroh> · <https://www.iroh.computer/docs/layers/relay>.

use iroh::endpoint::presets::Minimal;
use iroh::endpoint::{BindError, PortmapperConfig};
use iroh::{Endpoint, RelayMap, RelayMode, RelayUrlParseError};

use crate::endpoint::{ALPN, CONTROL_ALPN, PAIRING_ALPN};

/// Most relay peers this endpoint's home-relay status tracks at once. A relay map is small by
/// construction (one URL configured today) but the cap exists so a future multi-relay config
/// cannot grow this without limit (CLAUDE.md §3, "every collection has a named, checked cap").
pub const MAX_RELAY_PEERS: usize = 8;

/// Which relay this device syncs through. `url` is required — ADR 0024 removed the relay-off
/// mode, so an empty URL is a configuration error, not a way to fall back to LAN-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayConfig {
    /// The relay's URL, e.g. `https://relay.example.org`.
    pub url: String,
    /// Refused above [`MAX_RELAY_PEERS`].
    pub max_peers: usize,
}

/// Why building a relay endpoint failed. Every variant names what was attempted (CLAUDE.md §3);
/// this is validated, not asserted, because the config comes from `config.toml`/`--relay`, i.e.
/// external input, never a value this crate constructs itself.
#[derive(Debug)]
pub enum RelayError {
    /// `RelayConfig::url` was empty; ADR 0024 has no relay-off mode to fall back to.
    EmptyUrl,
    /// `RelayConfig::url` did not parse as a relay URL.
    InvalidUrl(RelayUrlParseError),
    /// `RelayConfig::max_peers` exceeded [`MAX_RELAY_PEERS`].
    TooManyPeers {
        /// The refused value.
        requested: usize,
    },
    /// The local endpoint could not bind.
    Bind(BindError),
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelayError::EmptyUrl => write!(f, "relay url is empty; ADR 0024 requires one"),
            RelayError::InvalidUrl(e) => write!(f, "relay url did not parse: {e}"),
            RelayError::TooManyPeers { requested } => write!(
                f,
                "max_peers {requested} exceeds the cap of {MAX_RELAY_PEERS}"
            ),
            RelayError::Bind(e) => write!(f, "bind the relay endpoint: {e}"),
        }
    }
}

impl std::error::Error for RelayError {}

/// Validates `cfg` and builds the `RelayMap` `build_endpoint` binds against — split out so a test
/// can assert "the configured relay set holds exactly this URL" without binding a real endpoint
/// (which would need a live relay to ever leave its initial, empty `home_relay_status()`).
pub(crate) fn relay_map(cfg: &RelayConfig) -> Result<RelayMap, RelayError> {
    if cfg.url.is_empty() {
        return Err(RelayError::EmptyUrl);
    }
    if cfg.max_peers > MAX_RELAY_PEERS {
        return Err(RelayError::TooManyPeers {
            requested: cfg.max_peers,
        });
    }
    RelayMap::try_from_iter([cfg.url.as_str()]).map_err(RelayError::InvalidUrl)
}

/// Shared by [`build_endpoint`] and the test-only insecure variant below: everything except
/// certificate verification, which the caller's builder closure sets.
async fn build(
    cfg: &RelayConfig,
    customize: impl FnOnce(iroh::endpoint::Builder) -> iroh::endpoint::Builder,
) -> Result<Endpoint, RelayError> {
    let map = relay_map(cfg)?;
    let builder = customize(
        Endpoint::builder(Minimal)
            .relay_mode(RelayMode::Custom(map))
            .portmapper_config(PortmapperConfig::Disabled),
    );
    let endpoint = builder.bind().await.map_err(RelayError::Bind)?;
    endpoint.set_alpns(vec![
        ALPN.to_vec(),
        PAIRING_ALPN.to_vec(),
        CONTROL_ALPN.to_vec(),
    ]);
    Ok(endpoint)
}

/// Binds an endpoint whose relay set is exactly `cfg.url` — never iroh's own `Default`/`Staging`
/// presets, which would silently route this device's traffic through n0's servers instead of the
/// relay this device was told to use. Port mapping stays disabled: routing through a configured
/// relay is not a reason to also ask the LAN router to open an external port (same reasoning as
/// `endpoint.rs`). Accepts [`ALPN`] and [`PAIRING_ALPN`] like the LAN endpoint, plus
/// [`CONTROL_ALPN`] (relay-only, task `daemon-workspace-identity-agreement` stage 5) — a caller
/// tells all three apart purely by which ALPN a connection negotiated. This crate's one production
/// constructor: certificate verification is never skipped.
pub async fn build_endpoint(cfg: &RelayConfig) -> Result<Endpoint, RelayError> {
    build(cfg, |b| b).await
}

/// Same as [`build_endpoint`], but binds with a caller-supplied identity seed instead of iroh's own
/// per-call random mint — task `daemon-workspace-identity-agreement` stage 1: a stable relay node
/// id across daemon restarts, needed so a peer's durably-stored `relay_node_id` (stage 2) stays
/// valid and a redial loop (stage 5) can keep reaching this device by the same identity.
pub async fn build_endpoint_with_secret_key(
    cfg: &RelayConfig,
    secret_key_bytes: [u8; 32],
) -> Result<Endpoint, RelayError> {
    build(cfg, |b| {
        b.secret_key(iroh::SecretKey::from_bytes(&secret_key_bytes))
    })
    .await
}

/// Test-only twin of [`build_endpoint`] that skips relay TLS certificate verification, so a test
/// can dial `iroh::test_utils::run_relay_server`'s self-signed local relay. `#[cfg(test)]` compiles
/// this out of every real build entirely — production code has no path to an insecure endpoint.
#[cfg(test)]
pub(crate) async fn build_endpoint_insecure_for_test(
    cfg: &RelayConfig,
) -> Result<Endpoint, RelayError> {
    build(cfg, |b| {
        b.ca_tls_config(iroh::tls::CaTlsConfig::insecure_skip_verify())
    })
    .await
}
