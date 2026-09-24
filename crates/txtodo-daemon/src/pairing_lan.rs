//! Daemon-to-daemon pairing relay over the LAN transport's own `Link`/`iroh` machinery, on a
//! dedicated ALPN (`txtodo_sync::PAIRING_ALPN`) so a pairing connection is never mistaken for the
//! group-keyed sync protocol `lan_session.rs` drives. Moves exactly the bytes
//! `txtodo_sync::PairingSession`'s crypto needs across the network: the joiner's ephemeral and
//! long-term public keys (joiner -> initiator), and the sealed `PairingGrant` once both sides
//! confirm (initiator -> joiner). Every message here is either already public or already sealed
//! by `PairingSession`'s own transcript-bound AEAD — this module adds no protection of its own.
//!
//! **Short bursts, not one held-open connection.** Matching `IrohLink::recv`'s `IDLE_TIMEOUT` and
//! `lan.rs`'s periodic-redial design, the joiner reconnects fresh for every retry rather than
//! holding one connection open across the human SAS-comparison pause (up to `PAIRING_WINDOW_MS`).
//! Each connection is one `JoinerHello`/`InitiatorReply` burst; the joiner retries on its own timer
//! (`RETRY_INTERVAL`) until a `Grant` arrives, it is `Rejected`, or the offer's own window elapses.

use std::sync::Arc;
use std::time::Duration;

use txtodo_model::DeviceId;
use txtodo_sync::{
    DiscoveredPeer, InitiatorReply, IrohLink, JoinerHello, LanEndpoint, Link, PAIRING_WINDOW_MS,
    PairingOffer, X25519_PUBLIC_KEY_BYTES,
};

use crate::lan_session::read;
use crate::pairing_lan_reject::{
    reject_handshake_failed, reject_no_active_session, reject_peer_conflict,
    reject_protocol_mismatch,
};
use crate::pairing_state::Role;
use crate::server::SharedWorkspace;
use crate::workspace::Workspace;

/// Gap between the joiner's connection attempts (see the module doc).
const RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// Gap between polls for the initiator's address before it resolves over mDNS.
const DISCOVER_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// [`DISCOVER_POLL_INTERVAL`] ticks to wait for this device's own LAN endpoint before giving up.
const ENDPOINT_WAIT_ATTEMPTS: u32 = 40;

/// Starts the joiner's background relay task: finds the initiator on the LAN, sends its identity
/// and keys, and retries until a grant arrives, it is rejected, or `PAIRING_WINDOW_MS` elapses.
pub(crate) fn spawn_joiner(
    ws: SharedWorkspace,
    offer: PairingOffer,
    own_public: [u8; X25519_PUBLIC_KEY_BYTES],
) {
    tokio::spawn(run_joiner(ws, offer, own_public));
}

/// Everything the joiner's retry loop needs, bundled to stay under `maxParams`.
struct JoinerCtx {
    ws: SharedWorkspace,
    offer: PairingOffer,
    own_device: DeviceId,
    own_public: [u8; X25519_PUBLIC_KEY_BYTES],
    static_public: [u8; txtodo_sync::DEVICE_STATIC_KEY_BYTES],
}

async fn run_joiner(
    ws: SharedWorkspace,
    offer: PairingOffer,
    own_public: [u8; X25519_PUBLIC_KEY_BYTES],
) {
    // No LAN endpoint at all is only a hard stop when the offer has no relay rendezvous either —
    // under `--no-lan` (or any device that never bound one), a relay-only offer must still reach
    // `joiner_loop` rather than bailing out here, which would make the relay path unreachable.
    let endpoint = wait_for_endpoint(&ws).await;
    if endpoint.is_none() && offer.relay_node_id.is_none() {
        log_no_endpoint(offer.device);
        return;
    }
    let (own_device, static_public) = {
        let ws = read(&ws);
        (ws.device(), ws.device_static_public().to_bytes())
    };
    let deadline_ms = offer.issued_at_ms.saturating_add(PAIRING_WINDOW_MS);
    let ctx = JoinerCtx {
        ws,
        offer,
        own_device,
        own_public,
        static_public,
    };
    joiner_loop(&ctx, endpoint.as_deref(), deadline_ms).await;
}

async fn joiner_loop(ctx: &JoinerCtx, endpoint: Option<&LanEndpoint>, deadline_ms: u64) {
    loop {
        if read(&ctx.ws).clock().now_ms() >= deadline_ms {
            log_window_expired(ctx.offer.device);
            return;
        }
        // A missing LAN sighting no longer stalls this loop outright: an offer carrying a relay
        // rendezvous still has something to try (`pairing_relay_dial::joiner_round`'s relay
        // half) — only "neither carrier has anything to dial yet" sleeps and retries.
        let peer = read(&ctx.ws).pairing_lan().find(ctx.offer.device);
        if peer.is_none() && ctx.offer.relay_node_id.is_none() {
            tokio::time::sleep(DISCOVER_POLL_INTERVAL).await;
            continue;
        }
        let hello = build_hello(ctx);
        let stop = crate::pairing_relay_dial::joiner_round(
            &ctx.ws,
            &ctx.offer,
            endpoint,
            peer.as_ref(),
            hello,
        )
        .await;
        if stop {
            return;
        }
        tokio::time::sleep(RETRY_INTERVAL).await;
    }
}

fn build_hello(ctx: &JoinerCtx) -> JoinerHello {
    let now_ms = read(&ctx.ws).clock().now_ms();
    let (confirmed, own_device) = read(&ctx.ws)
        .pairing()
        .joiner_local_confirmed(now_ms)
        .unwrap_or((false, false));
    JoinerHello {
        device: ctx.own_device,
        group: ctx.offer.group,
        nonce: ctx.offer.nonce,
        public_key: ctx.own_public,
        static_public: ctx.static_public,
        confirmed,
        own_device,
    }
}

fn log_no_endpoint(peer: DeviceId) {
    tracing::warn!(%peer, "pairing_joiner_no_lan_endpoint");
}

fn log_window_expired(peer: DeviceId) {
    tracing::warn!(%peer, "pairing_joiner_window_expired");
}

pub(crate) fn log_joiner_rejected(peer: DeviceId) {
    tracing::warn!(%peer, "pairing_joiner_rejected");
}

/// `pub(crate)`: `pairing_relay_dial.rs`'s racing joiner round calls this on a `Grant` reply.
pub(crate) fn finish_joiner(ws: &SharedWorkspace, offer: &PairingOffer, sealed: &[u8]) {
    let now_ms = read(ws).clock().now_ms();
    // This device's own session can only learn the initiator confirmed through this message: a
    // non-empty `Grant` is only possible once the initiator's session was ready, so it is the proof.
    // The initiator's own-device answer arrives inside the grant (`adopt_group_key`), not here.
    let _ = read(ws).pairing().mark_remote_confirmed(now_ms, false);
    match read(ws).adopt_group_key(offer.group, sealed, now_ms) {
        Ok(()) => {
            log_joiner_adopted(offer.device);
            record_offer_relay_reachability(ws, offer);
        }
        Err(e) => log_joiner_adopt_failed(offer.device, &e),
    }
}

/// Best-effort, logged only: records the initiator's relay node id/URL (if the offer carried one)
/// against its device row. Only that direction — the reverse is a documented gap, not a bug.
pub(crate) fn record_offer_relay_reachability(ws: &SharedWorkspace, offer: &PairingOffer) {
    let (Some(relay_node_id), Some(relay_url)) = (offer.relay_node_id, &offer.relay_url) else {
        return;
    };
    if let Err(e) = read(ws).record_peer_relay_reachability(offer.device, relay_node_id, relay_url)
    {
        tracing::warn!(peer = %offer.device, error = %e, "pairing_joiner_record_relay_reachability_failed");
    }
}

fn log_joiner_adopted(peer: DeviceId) {
    tracing::info!(%peer, "pairing_joiner_group_key_adopted");
}

fn log_joiner_adopt_failed(peer: DeviceId, e: &crate::pairing_state_error::PairingStateError) {
    tracing::warn!(%peer, error = %e, "pairing_joiner_adopt_group_key_failed");
}

async fn wait_for_endpoint(ws: &SharedWorkspace) -> Option<Arc<LanEndpoint>> {
    for _ in 0..ENDPOINT_WAIT_ATTEMPTS {
        if let Some(e) = read(ws).pairing_lan().endpoint() {
            return Some(e);
        }
        tokio::time::sleep(DISCOVER_POLL_INTERVAL).await;
    }
    None
}

/// One connection attempt over LAN: dial, send `hello`, read one reply. `None` on any failure
/// worth a retry — logged at `info`, not `debug`: pairing is a rare, human-paced ceremony, and a
/// joiner whose rounds fail silently looks exactly like an initiator that never confirmed.
fn log_lan_connect_failed(peer: DeviceId, e: &txtodo_sync::LanError) {
    tracing::info!(%peer, error = %e, "pairing_joiner_lan_connect_failed");
}

fn log_lan_round_no_reply(peer: DeviceId) {
    tracing::info!(%peer, "pairing_joiner_lan_round_no_reply");
}

pub(crate) async fn attempt(
    endpoint: &LanEndpoint,
    peer: &DiscoveredPeer,
    hello: JoinerHello,
) -> Option<InitiatorReply> {
    let link = match endpoint.connect_pairing(peer.node, &peer.addresses).await {
        Ok(link) => link,
        Err(e) => {
            log_lan_connect_failed(peer.device, &e);
            return None;
        }
    };
    let reply = tokio::task::spawn_blocking(move || send_and_receive(link, &hello))
        .await
        .ok()
        .flatten();
    if reply.is_none() {
        log_lan_round_no_reply(peer.device);
    }
    reply
}

/// Blocks a dedicated thread on `link`'s synchronous `send`/`recv`; `pairing_relay_dial.rs` too.
pub(crate) fn send_and_receive(mut link: IrohLink, hello: &JoinerHello) -> Option<InitiatorReply> {
    let frame = hello.encode().ok()?;
    link.send(frame).ok()?;
    let reply_frame = link.recv().ok()?;
    InitiatorReply::decode(&reply_frame).ok()
}

/// Handles one incoming pairing connection over the LAN carrier — [`handle_incoming_over`]'s
/// `"lan"` twin. `Link::send`/`recv` block, so this must never run on a plain tokio task.
pub(crate) fn handle_incoming(ws: &SharedWorkspace, link: &mut dyn Link) {
    handle_incoming_over(ws, link, "lan");
}

/// [`handle_incoming`], naming which carrier this connection arrived over — `relay.rs`'s own
/// accept loop calls this with `"relay"`. Records (and logs, at `info`) the carrier only when this
/// round actually finalizes the pairing (`InitiatorReply::Grant`) — never optimistically.
pub(crate) fn handle_incoming_over(
    ws: &SharedWorkspace,
    link: &mut dyn Link,
    carrier: &'static str,
) {
    let Ok(frame) = link.recv() else {
        return;
    };
    let Ok(hello) = JoinerHello::decode(&frame) else {
        tracing::debug!("pairing_initiator_bad_hello_frame");
        return;
    };
    let peer = hello.device;
    let reply = process_hello(ws, hello);
    if matches!(reply, InitiatorReply::Grant(_)) {
        read(ws).pairing_lan().record_carrier(carrier);
    }
    let Ok(reply_frame) = reply.encode() else {
        return;
    };
    // `finish` before the link drops: over a relay the drop's QUIC close beat the reply every time.
    let delivered = link.send(reply_frame).and_then(|()| link.finish()).is_ok();
    log_reply(peer, carrier, &reply, delivered);
}

/// `info` for every reply, not just `Grant`: a `Pending`-answering initiator used to be silent,
/// indistinguishable in its own log from one that never heard the joiner at all.
fn log_reply(peer: DeviceId, carrier: &'static str, reply: &InitiatorReply, delivered: bool) {
    let reply = match reply {
        InitiatorReply::Pending => "pending",
        InitiatorReply::Rejected => "rejected",
        InitiatorReply::Grant(_) => "grant",
    };
    tracing::info!(%peer, carrier, reply, delivered, "pairing_initiator_replied");
}

/// `pub(crate)`: `pairing_lan_tests.rs` drives this directly — each `reject_*` below now logs its
/// own reason, previously a silent `InitiatorReply::Rejected`.
pub(crate) fn process_hello(ws: &SharedWorkspace, hello: JoinerHello) -> InitiatorReply {
    let ws = read(ws);
    // Checked before requiring an active session: a successful finalize clears it (see
    // `finalize_or_pending`'s doc), so a retry after that would otherwise see `NotActive`. The
    // cache is device-global (`pairing()`, not the per-workspace `pairing_lan()`): the relay
    // accept path routes each round to an arbitrary open workspace, so a per-workspace cache
    // missed whenever a retry landed on a different one and this device hard-rejected the joiner.
    if let Some((sealed, own)) = ws.pairing().cached_grant(hello.device, hello.nonce) {
        // `register_device` is an upsert, so retrying this on every retried hello (not just the
        // first) is safe and is how a transient failure below gets another chance instead of
        // being silently dropped forever — see `register_joiner_device`'s own doc.
        let joiner = (hello.device, hello.static_public, own);
        register_joiner_device(&ws, joiner, ws.clock().now_ms());
        return InitiatorReply::Grant(sealed);
    }
    let now_ms = ws.clock().now_ms();
    let pairing = ws.pairing();
    let snapshot = match pairing.snapshot(now_ms) {
        Ok(s) => s,
        Err(e) => return reject_no_active_session(hello.device, &e),
    };
    if snapshot.role != Role::Initiator
        || snapshot.group != hello.group
        || snapshot.nonce != hello.nonce
    {
        return reject_protocol_mismatch(hello.device);
    }
    if snapshot.is_handshaken {
        // A second device claiming a nonce/group already bound to someone else: refuse.
        if snapshot.peer_device != Some(hello.device) {
            return reject_peer_conflict(hello.device, snapshot.peer_device);
        }
    } else if let Err(e) = pairing.complete_as_initiator(hello.device, hello.public_key, now_ms) {
        return reject_handshake_failed(hello.device, &e);
    }
    if hello.confirmed {
        let _ = pairing.mark_remote_confirmed(now_ms, hello.own_device);
    }
    finalize_or_pending(&ws, hello.device, hello.static_public, now_ms)
}

/// `error`: on the wire this is just `Pending`, so without it a keystore/crypto failure leaves
/// the joiner retrying with no visible cause.
fn log_finalize_failed(peer: DeviceId, e: &crate::pairing_state_error::PairingStateError) {
    tracing::error!(%peer, error = %e, "pairing_initiator_finalize_failed");
}

/// `try_finalize_initiator` caches the sealed grant device-globally on success (keyed by the
/// session's own peer/nonce, atomically with clearing `active`), so a retried `JoinerHello` is
/// re-served it by `process_hello`'s top check above — no caching is done here.
fn finalize_or_pending(
    ws: &Workspace,
    device: DeviceId,
    static_public: [u8; txtodo_sync::DEVICE_STATIC_KEY_BYTES],
    now_ms: u64,
) -> InitiatorReply {
    let sealed = match ws.pairing().try_finalize_initiator(
        ws.key_store().as_ref(),
        ws.device_static_public(),
        now_ms,
    ) {
        Ok(sealed) => sealed,
        Err(e) => {
            log_finalize_failed(device, &e);
            None
        }
    };
    match sealed {
        Some((sealed, own)) => {
            register_joiner_device(ws, (device, static_public, own), now_ms);
            InitiatorReply::Grant(sealed)
        }
        None => InitiatorReply::Pending,
    }
}

/// Registers the joiner's static key (carried on every `JoinerHello`, including retries) in the
/// initiator's `devices` table. Deliberately reads `static_public` straight off the hello rather
/// than from `active` — by the time `try_finalize_initiator` returns success it has already
/// cleared `active`, so a previous version of this read `PairingRegistry::peer_static` here and
/// always got `None`: registration never actually ran. `register_device` is an upsert, so calling
/// this again on a retried/cached hello (see `process_hello`) is safe and is how a failure here
/// gets another chance instead of being silently and permanently dropped.
/// `joiner` is the device, its static key and whether both humans called each other own.
fn register_joiner_device(
    ws: &Workspace,
    joiner: (DeviceId, [u8; txtodo_sync::DEVICE_STATIC_KEY_BYTES], bool),
    now_ms: u64,
) {
    let (device, static_public, own) = joiner;
    if let Err(e) = ws.register_paired_device(device, static_public, now_ms, own) {
        tracing::warn!(peer = %device, error = %e, "pairing_initiator_register_joiner_failed");
    }
}
