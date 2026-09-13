//! The one iroh QUIC endpoint constructor this crate has (plan M4 `sync-lan-transport`). Nothing
//! else touches `iroh` directly — check `.claude/budgets.json`'s `allowedDeps` before wiring
//! anything else to it.
//!
//! **Relay off is a thing to prove, not just not-configure.** M8 turns the relay on deliberately;
//! until then, a stray default must never ship LAN traffic through a third party. iroh's own
//! `presets::N0` (the convenience default shown in its own docs) enables a public relay *and* a
//! DNS-based address-lookup service — exactly the kind of default this crate cannot inherit
//! silently. `presets::Minimal` sets nothing but the mandatory TLS crypto provider, so
//! `RelayMode::Disabled` below is not "relying on a preset that happens to default to off" — it is
//! the only source of that fact, asserted by `endpoint_tests.rs`'s relay-empty test so an iroh
//! upgrade that changes a default fails here, not silently.
//!
//! **Port mapping is the same class of stray default, found while wiring `txtodo-daemon` to this
//! module.** `presets::Minimal` leaves `PortmapperConfig` at its crate-wide default
//! (`Enabled`), which sends SSDP multicast to the LAN's router asking it to open an external
//! port — exactly the kind of "ask a third party to help" behaviour a pure-LAN mode must not do
//! either, and iroh's own doc on `PortmapperConfig::Disabled` names the actual cost: "can trigger
//! firewall prompts on some networks". `RelayMode::Disabled` does not touch this setting, so it
//! needed its own explicit disable, same reasoning as relay.
//! Refs: <https://docs.rs/iroh> · <https://www.iroh.computer/docs>.

use iroh::endpoint::presets::Minimal;
use iroh::endpoint::{BindError, PortmapperConfig};
use iroh::{Endpoint, RelayMode};

/// Identifies this protocol on the wire, so an iroh endpoint used for something else on the same
/// machine can never accidentally accept (or be mistaken for) a txtodo connection.
pub const ALPN: &[u8] = b"txtodo/sync/1";

/// Binds the one endpoint shape this crate ever constructs: LAN-only, no relay, no port mapping,
/// no third-party address lookup. Picks an ephemeral local UDP port on every interface.
pub async fn bind_local_endpoint() -> Result<Endpoint, BindError> {
    let endpoint = Endpoint::builder(Minimal)
        .relay_mode(RelayMode::Disabled)
        .portmapper_config(PortmapperConfig::Disabled)
        .bind()
        .await?;
    endpoint.set_alpns(vec![ALPN.to_vec()]);
    Ok(endpoint)
}
