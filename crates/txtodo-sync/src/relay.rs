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

use crate::endpoint::{ALPN, PAIRING_ALPN};

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

/// Binds an endpoint whose relay set is exactly `cfg.url` — never iroh's own `Default`/`Staging`
/// presets, which would silently route this device's traffic through n0's servers instead of the
/// relay this device was told to use. Port mapping stays disabled: routing through a configured
/// relay is not a reason to also ask the LAN router to open an external port (same reasoning as
/// `endpoint.rs`). Accepts both [`ALPN`] and [`PAIRING_ALPN`], same as the LAN endpoint, so a
/// caller can tell a sync connection from a pairing one purely by which ALPN it negotiated.
pub async fn build_endpoint(cfg: &RelayConfig) -> Result<Endpoint, RelayError> {
    let map = relay_map(cfg)?;
    let endpoint = Endpoint::builder(Minimal)
        .relay_mode(RelayMode::Custom(map))
        .portmapper_config(PortmapperConfig::Disabled)
        .bind()
        .await
        .map_err(RelayError::Bind)?;
    endpoint.set_alpns(vec![ALPN.to_vec(), PAIRING_ALPN.to_vec()]);
    Ok(endpoint)
}
