//! Daemon-to-daemon pairing relay over the LAN transport's own `Link`/`iroh` machinery
//! (`sync-lan-transport`), on a dedicated ALPN (`txtodo_sync::PAIRING_ALPN`) so a pairing
//! connection is never mistaken for the group-keyed sync protocol `lan_session.rs` drives. Moves
//! exactly the bytes `txtodo_sync::PairingSession`'s crypto needs across the network: the joiner's
//! ephemeral and long-term public keys (joiner -> initiator), and the sealed `PairingGrant` once
//! both sides confirm (initiator -> joiner). Every message here is either already public (no more
//! secret than the QR/offer's own `x25519_pub`) or already sealed by `PairingSession`'s own
//! transcript-bound AEAD — this module adds no protection of its own, only moves bytes, the same
//! stance `lan_session.rs`'s module doc takes for the group-sync `Message` protocol.
//!
//! **Short bursts, not one held-open connection.** Matching `IrohLink::recv`'s `IDLE_TIMEOUT`
//! (`txtodo-sync`'s own doc) and `lan.rs`'s periodic-redial design, the joiner reconnects fresh for
//! every retry rather than holding one connection open across the human SAS-comparison pause
//! (which can take up to `PAIRING_WINDOW_MS`) — a held-open connection would simply be reported
//! `Closed` by `IrohLink` after 750 ms of silence anyway. Each connection is one
//! `JoinerHello`/`InitiatorReply` request-response burst; the joiner retries on its own timer
//! (`RETRY_INTERVAL`) until a `Grant` arrives, it is `Rejected`, or the offer's own window elapses.

use std::sync::Arc;
use std::time::Duration;

use txtodo_model::DeviceId;
use txtodo_sync::{
    DiscoveredPeer, InitiatorReply, IrohLink, JoinerHello, LanEndpoint, Link, PAIRING_WINDOW_MS,
    PairingOffer, X25519_PUBLIC_KEY_BYTES,
};

use crate::lan_session::read;
use crate::pairing_state::Role;
use crate::server::SharedWorkspace;
use crate::workspace::Workspace;

/// Gap between the joiner's connection attempts — short bursts, not one held-open connection (see
/// the module doc). Long enough that a human's SAS comparison does not flood the LAN with dials.
const RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// Gap between polls for the initiator's address before it has been resolved over mDNS yet.
const DISCOVER_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// How many [`DISCOVER_POLL_INTERVAL`] ticks to wait for this device's own LAN endpoint to be
/// bound before giving up — `lan.rs` binds it once, early, but pairing can start before that
/// finishes.
const ENDPOINT_WAIT_ATTEMPTS: u32 = 40;

/// Starts the joiner's background relay task: finds the initiator on the LAN, sends this device's
/// identity and keys, and retries until a grant arrives, the attempt is rejected, or the offer's
/// own `PAIRING_WINDOW_MS` elapses. Spawned by `pairing_grpc.rs::pair_accept_impl` right after the
/// local handshake (SAS already computed — see `txtodo_sync::PairingSession::accept`'s own doc on
/// why the joiner needs no network round trip for that part).
pub(crate) fn spawn_joiner(
    ws: SharedWorkspace,
    offer: PairingOffer,
    own_public: [u8; X25519_PUBLIC_KEY_BYTES],
) {
    tokio::spawn(run_joiner(ws, offer, own_public));
}

/// Everything the joiner's retry loop needs, bundled so no function below needs more than
/// `maxParams` arguments.
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
    let Some(endpoint) = wait_for_endpoint(&ws).await else {
        log_no_endpoint(offer.device);
        return;
    };
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
    joiner_loop(&ctx, &endpoint, deadline_ms).await;
}

async fn joiner_loop(ctx: &JoinerCtx, endpoint: &LanEndpoint, deadline_ms: u64) {
    loop {
        if read(&ctx.ws).clock().now_ms() >= deadline_ms {
            log_window_expired(ctx.offer.device);
            return;
        }
        let Some(peer) = read(&ctx.ws).pairing_lan().find(ctx.offer.device) else {
            tokio::time::sleep(DISCOVER_POLL_INTERVAL).await;
            continue;
        };
        let hello = build_hello(ctx);
        if joiner_round(ctx, endpoint, &peer, hello).await {
            return;
        }
        tokio::time::sleep(RETRY_INTERVAL).await;
    }
}

/// One connection attempt and the reaction to it. Returns `true` when the loop should stop (a
/// grant landed, or the attempt was rejected outright), `false` to sleep and retry.
async fn joiner_round(
    ctx: &JoinerCtx,
    endpoint: &LanEndpoint,
    peer: &DiscoveredPeer,
    hello: JoinerHello,
) -> bool {
    match attempt(endpoint, peer, hello).await {
        Some(InitiatorReply::Grant(sealed)) => {
            finish_joiner(&ctx.ws, &ctx.offer, &sealed);
            true
        }
        Some(InitiatorReply::Rejected) => {
            log_joiner_rejected(ctx.offer.device);
            true
        }
        Some(InitiatorReply::Pending) | None => false,
    }
}

fn build_hello(ctx: &JoinerCtx) -> JoinerHello {
    let now_ms = read(&ctx.ws).clock().now_ms();
    let confirmed = read(&ctx.ws)
        .pairing()
        .joiner_local_confirmed(now_ms)
        .unwrap_or(false);
    JoinerHello {
        device: ctx.own_device,
        group: ctx.offer.group,
        nonce: ctx.offer.nonce,
        public_key: ctx.own_public,
        static_public: ctx.static_public,
        confirmed,
    }
}

fn log_no_endpoint(peer: DeviceId) {
    tracing::warn!(%peer, "pairing_joiner_no_lan_endpoint");
}

fn log_window_expired(peer: DeviceId) {
    tracing::warn!(%peer, "pairing_joiner_window_expired");
}

fn log_joiner_rejected(peer: DeviceId) {
    tracing::warn!(%peer, "pairing_joiner_rejected");
}

fn finish_joiner(ws: &SharedWorkspace, offer: &PairingOffer, sealed: &[u8]) {
    let now_ms = read(ws).clock().now_ms();
    // `is_ready_to_send_key` (which `adopt_group_key` requires) reads *this* device's own
    // `PairingSession`, which has no way to observe the initiator's local confirmation except
    // through this very message: receiving a non-empty `Grant` at all is only possible once the
    // initiator's own session was ready to send one (`process_hello`/`finalize_or_pending`'s own
    // gate), so it doubles as that proof for the joiner's side.
    let _ = read(ws).pairing().mark_remote_confirmed(now_ms);
    match read(ws).adopt_group_key(offer.group, sealed, now_ms) {
        Ok(()) => log_joiner_adopted(offer.device),
        Err(e) => log_joiner_adopt_failed(offer.device, &e),
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

/// One connection attempt: dial, send `hello`, read one reply. `None` on any failure worth a retry
/// (dial refused, link closed, a frame that didn't decode) — never a hard error, since the peer
/// may simply not be reachable yet.
async fn attempt(
    endpoint: &LanEndpoint,
    peer: &DiscoveredPeer,
    hello: JoinerHello,
) -> Option<InitiatorReply> {
    let link = endpoint
        .connect_pairing(peer.node, &peer.addresses)
        .await
        .ok()?;
    tokio::task::spawn_blocking(move || send_and_receive(link, &hello))
        .await
        .ok()
        .flatten()
}

/// Blocks a dedicated thread on `link`'s synchronous `send`/`recv` (`Link`'s own contract — see
/// `lan_link.rs`'s module doc on why this must never run on a plain tokio task).
fn send_and_receive(mut link: IrohLink, hello: &JoinerHello) -> Option<InitiatorReply> {
    let frame = hello.encode().ok()?;
    link.send(frame).ok()?;
    let reply_frame = link.recv().ok()?;
    InitiatorReply::decode(&reply_frame).ok()
}

/// Handles one incoming pairing connection (this device as initiator): reads one `JoinerHello`,
/// advances this daemon's `PairingRegistry` as far as it will go, and replies once. Spawned by
/// `lan.rs`'s accept loop on a `spawn_blocking` thread, same as `lan_session::drive_session` for a
/// normal sync connection — `Link::send`/`recv` block, so this must never run on a plain tokio task.
pub(crate) fn handle_incoming(ws: &SharedWorkspace, link: &mut dyn Link) {
    let Ok(frame) = link.recv() else {
        return;
    };
    let Ok(hello) = JoinerHello::decode(&frame) else {
        tracing::debug!("pairing_initiator_bad_hello_frame");
        return;
    };
    let reply = process_hello(ws, hello);
    let Ok(reply_frame) = reply.encode() else {
        return;
    };
    let _ = link.send(reply_frame);
}

fn process_hello(ws: &SharedWorkspace, hello: JoinerHello) -> InitiatorReply {
    let ws = read(ws);
    // Checked *before* requiring an active `PairingRegistry` session: a successful finalize
    // clears that session (see `finalize_or_pending`'s doc), so a retry arriving after it would
    // otherwise see `NotActive` and be rejected despite a perfectly valid cached grant.
    if let Some(sealed) = ws.pairing_lan().cached_grant(hello.device, hello.nonce) {
        return InitiatorReply::Grant(sealed);
    }
    let now_ms = ws.clock().now_ms();
    let pairing = ws.pairing();
    let Ok(snapshot) = pairing.snapshot(now_ms) else {
        return InitiatorReply::Rejected;
    };
    if snapshot.role != Role::Initiator
        || snapshot.group != hello.group
        || snapshot.nonce != hello.nonce
    {
        return InitiatorReply::Rejected;
    }
    if snapshot.is_handshaken {
        // A second device claiming the same nonce/group after the handshake already bound to
        // someone else: refuse rather than let a later arrival silently take over.
        if snapshot.peer_device != Some(hello.device) {
            return InitiatorReply::Rejected;
        }
    } else if pairing
        .complete_as_initiator(hello.device, hello.public_key, now_ms)
        .is_err()
    {
        return InitiatorReply::Rejected;
    }
    let _ = pairing.set_peer_static(hello.static_public, now_ms);
    if hello.confirmed {
        let _ = pairing.mark_remote_confirmed(now_ms);
    }
    finalize_or_pending(&ws, hello.device, hello.nonce, now_ms)
}

fn finalize_or_pending(
    ws: &Workspace,
    device: DeviceId,
    nonce: txtodo_sync::Nonce,
    now_ms: u64,
) -> InitiatorReply {
    if let Some(sealed) = ws.pairing_lan().cached_grant(device, nonce) {
        return InitiatorReply::Grant(sealed);
    }
    let sealed = ws
        .pairing()
        .try_finalize_initiator(ws.key_store().as_ref(), ws.device_static_public(), now_ms)
        .unwrap_or(None);
    match sealed {
        Some(sealed) => {
            ws.pairing_lan().cache_grant(device, nonce, sealed.clone());
            register_joiner_device(ws, device, now_ms);
            InitiatorReply::Grant(sealed)
        }
        None => InitiatorReply::Pending,
    }
}

/// Registers the joiner's long-term static key in this (initiator's) own `devices` table — the
/// reverse leg of `adopt_group_key`'s doc: symmetric with the joiner registering the initiator's
/// key from the sealed grant. Best-effort and logged only: a failure here does not affect the
/// group key the joiner already received, and a missing row can always be re-derived by re-pairing.
fn register_joiner_device(ws: &Workspace, device: DeviceId, now_ms: u64) {
    let Some(static_public) = ws.pairing().peer_static(now_ms) else {
        return;
    };
    if let Err(e) = ws.register_paired_device(device, static_public, now_ms) {
        tracing::warn!(peer = %device, error = %e, "pairing_initiator_register_joiner_failed");
    }
}

impl Workspace {
    /// Registers a peer's long-term static public key in this workspace's own `devices` table
    /// (plan M4 `sync-device-remove`) — the initiator's side of pairing's static-key exchange,
    /// symmetric with [`Workspace::adopt_group_key`]'s joiner-side registration. Split out here
    /// (rather than `workspace.rs`, over its file budget) same as `device_remove.rs`'s own
    /// external `impl Workspace` block.
    pub(crate) fn register_paired_device(
        &self,
        device: DeviceId,
        static_public: [u8; txtodo_sync::DEVICE_STATIC_KEY_BYTES],
        now_ms: u64,
    ) -> Result<(), txtodo_store::StoreError> {
        let mut store = self
            .store()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.register_device(&txtodo_store::NewDevice {
            device,
            name: String::new(),
            static_public,
            paired_at_ms: now_ms,
            last_known_wall_ms: None,
            key_epoch: 0,
        })
    }
}
