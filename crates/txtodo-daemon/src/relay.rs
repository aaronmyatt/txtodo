//! Wires `txtodo_sync::RelayEndpoint` into a running `txtodod` (plan M8 `sync-relay-enable`,
//! ADR 0026): binds the relay-configured endpoint when `--relay <url>` is set and runs an accept
//! loop, updating `LanStatus`'s relay fields on every bind/accept outcome. Twin of `lan.rs`, but
//! relay is optional and, per ADR 0026, purely additive — LAN stays the primary path. This module
//! only binds the endpoint and accepts incoming relay connections; the *dialing* half of the
//! fallback (this device reaching a peer over relay when LAN can't) is `lan.rs::dial_and_spawn`,
//! via `relay_fallback.rs` and `RelayState`.
//!
//! **Known scope limit, deliberate**: unlike `lan.rs::rebuild_on_group_change`, this module never
//! rebinds anything when the workspace's sync group changes (e.g. mid-run pairing). Pairing-over-
//! relay is a separate, not-yet-built task (`sync-pairing-relay`, scoped by ADR 0026's own
//! follow-up list) — out of scope here. A relay endpoint bound before a pairing completes will
//! simply keep gating `connect`/`accept` on the group it was bound with.

use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use txtodo_model::DeviceId;
use txtodo_sync::{GroupId, HolepunchError, MAX_RELAY_PEERS, RelayConfig, RelayEndpoint};

use crate::lan::{MAX_CONCURRENT_LAN_SESSIONS, spawn_driver};
use crate::lan_session::read;
use crate::lan_status::LanStatus;
use crate::server::SharedWorkspace;

/// This device's identity plus its workspace and live status — bundled the same way `lan.rs`'s own
/// `LanCtx` is, so no function below needs more than `maxParams` arguments.
#[derive(Clone)]
struct RelayCtx {
    ws: SharedWorkspace,
    device: DeviceId,
    group: GroupId,
    status: LanStatus,
}

/// The background relay transport task; `abort()` on daemon shutdown, same pattern as
/// `LanTransport`.
pub struct RelayTransport {
    task: JoinHandle<()>,
}

impl RelayTransport {
    /// Stops the relay transport. Best-effort: the task may already have exited (bind failed).
    pub fn abort(&self) {
        self.task.abort();
    }
}

/// Starts the relay endpoint as a background task when `relay_url` names one; `None` (or empty)
/// means relay stays off, matching M4/ADR-0024-era behaviour with no fallback carrier at all —
/// `LanStatus::set_relay_configured("")` records that fact for `Health` without spawning anything.
pub fn start(ws: SharedWorkspace, relay_url: Option<String>) -> Option<RelayTransport> {
    let url = relay_url.unwrap_or_default();
    let ctx = {
        let guard = read(&ws);
        RelayCtx {
            ws: ws.clone(),
            device: guard.device(),
            group: guard.group(),
            status: guard.lan_status().clone(),
        }
    };
    ctx.status.set_relay_configured(&url);
    if url.is_empty() {
        return None;
    }
    Some(RelayTransport {
        task: tokio::spawn(run(ctx, url)),
    })
}

async fn bind(ctx: &RelayCtx, url: String) -> Option<Arc<RelayEndpoint>> {
    let cfg = RelayConfig {
        url,
        max_peers: MAX_RELAY_PEERS,
    };
    match RelayEndpoint::bind(&cfg, ctx.group).await {
        Ok(e) => {
            ctx.status
                .set_relay_last_outcome("bound; awaiting connections");
            let endpoint = Arc::new(e);
            read(&ctx.ws).relay_state().set(Arc::clone(&endpoint));
            Some(endpoint)
        }
        Err(e) => {
            tracing::warn!(error = %e, "relay_bind_failed_running_without_relay_fallback");
            ctx.status
                .set_relay_last_outcome(format!("bind failed: {e}"));
            None
        }
    }
}

/// Bounded the same way `lan.rs::accept_one` is: a relay endpoint flooded with connections must
/// not spawn unbounded tasks either. Pairing-over-relay does not exist yet (module doc), so every
/// accepted connection here is a sync session — no ALPN dispatch to a pairing driver.
fn on_accepted(link: txtodo_sync::IrohLink, sessions: &Arc<Semaphore>, ctx: &RelayCtx) {
    ctx.status.set_relay_last_outcome("accepted a connection");
    let Ok(permit) = Arc::clone(sessions).try_acquire_owned() else {
        tracing::warn!("relay_session_cap_reached_dropping_incoming");
        return;
    };
    spawn_driver(ctx.ws.clone(), ctx.device, ctx.group, link, permit);
}

fn on_no_incoming(ctx: &RelayCtx) {
    tracing::warn!("relay_endpoint_closed");
    ctx.status.set_relay_last_outcome("endpoint closed");
}

fn on_accept_err(e: &HolepunchError, ctx: &RelayCtx) {
    tracing::debug!(error = %e, "relay_accept_failed");
    ctx.status
        .set_relay_last_outcome(format!("accept failed: {e}"));
}

/// One accept iteration; `false` means the endpoint is done and `run`'s loop should stop.
async fn accept_once(endpoint: &RelayEndpoint, sessions: &Arc<Semaphore>, ctx: &RelayCtx) -> bool {
    match endpoint.accept().await {
        Ok(link) => {
            on_accepted(link, sessions, ctx);
            true
        }
        Err(HolepunchError::NoIncoming) => {
            on_no_incoming(ctx);
            false
        }
        Err(e) => {
            on_accept_err(&e, ctx);
            true
        }
    }
}

async fn run(ctx: RelayCtx, url: String) {
    let Some(endpoint) = bind(&ctx, url).await else {
        return;
    };
    let sessions = Arc::new(Semaphore::new(MAX_CONCURRENT_LAN_SESSIONS));
    while accept_once(&endpoint, &sessions, &ctx).await {}
}
