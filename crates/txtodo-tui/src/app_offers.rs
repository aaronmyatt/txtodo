//! `app.rs`'s status-tick half: the `o` pane's accept/decline actions and offer list (task
//! `workspace-offer-cli`) and the `s` indicator's `SyncStatus` refresh, both driven by
//! `refresh_on_tick`. Its own file only for `app.rs`'s line budget.

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};
use crate::state::{AppState, PeerStatus, SyncSnapshot};
use crate::state_offers::OfferItem;

/// Sends an accept or decline, then re-reads the pending list so the pane shows the result. An
/// accept adopts a workspace into the daemon's registry; the TUI keeps showing the one it opened.
pub async fn perform_accept(
    daemon: &mut Daemon,
    state: &mut AppState,
    req: pb::WorkspaceAcceptOfferRequest,
) -> Result<(), DaemonError> {
    daemon.workspace_accept_offer(req).await?;
    refresh_offers(daemon, state).await;
    Ok(())
}

/// See [`perform_accept`].
pub async fn perform_decline(
    daemon: &mut Daemon,
    state: &mut AppState,
    req: pb::WorkspaceDeclineOfferRequest,
) -> Result<(), DaemonError> {
    daemon.workspace_decline_offer(req).await?;
    refresh_offers(daemon, state).await;
    Ok(())
}

/// The status tick: the `s` indicator and the pending offers together, so `run_loop_inner`'s
/// select arm stays one call.
pub async fn refresh_on_tick(daemon: &mut Daemon, state: &mut AppState) {
    refresh_sync_status(daemon, state).await;
    refresh_offers(daemon, state).await;
}

/// Best-effort refresh of the pending list — a failed call (an older daemon without the RPC, a
/// transient hiccup) leaves the previous list in place, same as `refresh_sync_status`.
pub async fn refresh_offers(daemon: &mut Daemon, state: &mut AppState) {
    if let Ok(offers) = daemon.workspace_pending_offers().await {
        state
            .offers
            .replace(offers.into_iter().map(to_offer_item).collect());
    }
}

fn to_offer_item(o: pb::PendingWorkspaceOffer) -> OfferItem {
    OfferItem {
        device: o.offering_device,
        workspace_id: o.workspace_id,
        name: o.name,
    }
}

/// Refreshes `state.sync` from a real `SyncStatus` call. Best-effort: a failed call (transient
/// daemon hiccup) leaves the previous snapshot in place rather than erroring the whole event
/// loop — the same "colours are never the only signal" spirit as `ui/sync.rs` itself, just applied
/// to a stale-but-present reading instead of a missing one.
pub async fn refresh_sync_status(daemon: &mut Daemon, state: &mut AppState) {
    if let Ok(resp) = daemon.sync_status().await {
        state.sync = to_sync_snapshot(resp);
    }
}

fn to_sync_snapshot(resp: pb::SyncStatusResponse) -> SyncSnapshot {
    SyncSnapshot {
        peers: resp
            .peers
            .into_iter()
            .map(|p| PeerStatus {
                device: p.device,
                lag_ms: p.lag_ms,
            })
            .collect(),
        pending_ops: u32::try_from(resp.pending_ops).unwrap_or(u32::MAX),
    }
}
