//! Relay carrier for the pairing joiner leg (plan M8 `sync-pairing-relay`, ADR 0026's follow-up
//! list): races LAN discovery against a relay dial gated on the pairing offer's own one-time
//! nonce, not `GroupId` — `tasks/sync-pairing-relay/notes.md`'s "Open design question", resolved
//! as option (a). The nonce gate is not new code here: `pairing_lan.rs::process_hello` already
//! refuses any `JoinerHello` whose `nonce`/`group` do not match this daemon's own active offer,
//! regardless of which carrier delivered it — `RelayEndpoint::connect_pairing` (txtodo-sync) has
//! no `GroupId` gate of its own (pairing has no shared group yet, the whole reason this task
//! exists), so that existing nonce check is the only gate that ever protected the LAN path either.
//! Reuses `pairing_lan.rs`'s `attempt`/`finish_joiner`/`log_joiner_rejected` and
//! `relay_fallback.rs`'s generic `lan_then_relay` race, so the only genuinely new code here is the
//! relay half of one dial attempt.

use std::time::Duration;

use txtodo_sync::{DiscoveredPeer, InitiatorReply, JoinerHello, LanEndpoint, PairingOffer};

use crate::lan_session::read;
use crate::pairing_lan::{attempt, finish_joiner, log_joiner_rejected, send_and_receive};
use crate::relay_fallback::lan_then_relay;
use crate::server::SharedWorkspace;

/// How long [`lan_then_relay`] gives the LAN dial before falling back to relay — short, since a
/// real LAN connect that is going to succeed at all does so quickly. Deliberately *not* also used
/// to bound the relay connect itself (see [`relay_attempt`]'s own doc): measured directly against
/// a real public relay, capping that connect at 3 s (or even `crate::lan::CONNECT_TIMEOUT`'s 10 s)
/// made every attempt fail just before completing, never once succeeding, the same "no timeout,
/// trust the outer retry loop" shape `pairing_lan.rs::attempt` already uses for the LAN connect.
const LAN_RACE_TIMEOUT: Duration = Duration::from_secs(3);

/// One joiner round: races a LAN dial (when a sighting exists) against a relay dial (when the
/// offer carries a relay rendezvous and this device has its own relay endpoint bound), same
/// "whichever connects first within a bounded timeout" shape `sync-relay-enable`'s ongoing-sync
/// carrier order already uses. `true` stops the retry loop (a grant landed or the attempt was
/// rejected outright), `false` retries.
pub(crate) async fn joiner_round(
    ws: &SharedWorkspace,
    offer: &PairingOffer,
    endpoint: Option<&LanEndpoint>,
    peer: Option<&DiscoveredPeer>,
    hello: JoinerHello,
) -> bool {
    let lan_dial = lan_attempt(ws, endpoint, peer, hello.clone());
    let relay_dial = relay_attempt(ws, offer, hello);
    match lan_then_relay(LAN_RACE_TIMEOUT, lan_dial, relay_dial).await {
        Some(InitiatorReply::Grant(sealed)) => {
            finish_joiner(ws, offer, &sealed);
            true
        }
        Some(InitiatorReply::Rejected) => {
            log_joiner_rejected(offer.device);
            true
        }
        Some(InitiatorReply::Pending) | None => false,
    }
}

/// The LAN half of the race: `None` outright when this device has no LAN endpoint at all (e.g.
/// `--no-lan`) or no sighting of this peer yet — the same "nothing to try" shape as the relay
/// half's own `None` cases below. Records `"lan"` as this pairing's carrier the moment a grant
/// actually arrives over it — never optimistically: a primary cancelled by [`lan_then_relay`]'s
/// own timeout never reaches this line at all, only a round that genuinely completed does.
async fn lan_attempt(
    ws: &SharedWorkspace,
    endpoint: Option<&LanEndpoint>,
    peer: Option<&DiscoveredPeer>,
    hello: JoinerHello,
) -> Option<InitiatorReply> {
    let reply = attempt(endpoint?, peer?, hello).await?;
    if matches!(reply, InitiatorReply::Grant(_)) {
        read(ws).pairing_lan().record_carrier("lan");
    }
    Some(reply)
}

/// The relay half: dials the offer's own relay rendezvous fields (the initiator's relay node id,
/// `notes.md`'s option (a)) over this device's own bound relay endpoint. `None` when either side
/// has nothing to try — the offer carries no relay rendezvous (the initiator never configured
/// one), or this device's own `relay.rs` has not bound an endpoint yet (relay never configured
/// here either) — exactly the "an offer with no relay configured pairs over LAN as today" case.
/// Deliberately no timeout wraps `connect_pairing` itself (see [`LAN_RACE_TIMEOUT`]'s own doc for
/// why): the joiner's outer retry loop (`pairing_lan.rs::joiner_loop`, bounded by
/// `PAIRING_WINDOW_MS`) is what recovers from a slow or hung relay attempt, exactly as it already
/// does for a slow or hung LAN one via `attempt`.
fn log_relay_no_rendezvous() {
    tracing::debug!("pairing_joiner_relay_no_rendezvous_in_offer");
}

fn log_relay_endpoint_not_bound() {
    tracing::debug!("pairing_joiner_relay_endpoint_not_bound");
}

fn log_relay_connect_failed(e: &txtodo_sync::HolepunchError) {
    tracing::debug!(error = %e, "pairing_joiner_relay_connect_failed");
}

fn log_relay_round_no_reply() {
    tracing::debug!("pairing_joiner_relay_round_no_reply");
}

/// The connect+dial half of [`relay_attempt`], split out purely to keep that function's own
/// cognitive complexity under this workspace's budget (`clippy.toml`) — logs (at `debug`, see
/// [`relay_attempt`]'s doc) every reason a round produced nothing, the same discipline
/// `pairing_lan.rs::attempt` now applies to its own LAN half.
async fn relay_connect(ws: &SharedWorkspace, offer: &PairingOffer) -> Option<txtodo_sync::IrohLink> {
    let Some(node) = offer.relay_node_id else {
        log_relay_no_rendezvous();
        return None;
    };
    let Some(endpoint) = read(ws).relay_state().get() else {
        log_relay_endpoint_not_bound();
        return None;
    };
    match endpoint.connect_pairing(node).await {
        Ok(link) => Some(link),
        Err(e) => {
            log_relay_connect_failed(&e);
            None
        }
    }
}

async fn relay_attempt(
    ws: &SharedWorkspace,
    offer: &PairingOffer,
    hello: JoinerHello,
) -> Option<InitiatorReply> {
    let link = relay_connect(ws, offer).await?;
    let reply = tokio::task::spawn_blocking(move || send_and_receive(link, &hello))
        .await
        .ok()
        .flatten();
    let Some(reply) = reply else {
        log_relay_round_no_reply();
        return None;
    };
    if matches!(reply, InitiatorReply::Grant(_)) {
        read(ws).pairing_lan().record_carrier("relay");
    }
    Some(reply)
}
