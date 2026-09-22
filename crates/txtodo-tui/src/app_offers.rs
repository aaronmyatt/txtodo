//! `app.rs`'s workspace-offer half (task `workspace-offer-cli`): performs the `o` pane's
//! accept/decline actions and refreshes its list on the status tick. Its own file only for
//! `app.rs`'s line budget.

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};
use crate::state::AppState;
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
    crate::app::refresh_sync_status(daemon, state).await;
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
