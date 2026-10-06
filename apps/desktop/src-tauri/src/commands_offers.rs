//! Workspace offers from paired devices (specs/client-parity.toml offers.*; the TUI's `o` pane):
//! list, accept, decline, for Settings > Devices. The bodies take any connected client, so the
//! e2e bridge runs the same code.

use crate::commands::ensure_connected;
use crate::daemon::DaemonClient;
use crate::dto::WorkspaceInfoDto;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, State};

/// One workspace a paired device offered, not answered yet.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OfferDto {
    /// The offering device (ULID text).
    pub offering_device: String,
    /// The offered workspace (ULID text), owned by that device.
    pub workspace_id: String,
    /// Its label; never identity.
    pub name: String,
    /// The offering device's clock at the offer, unix ms.
    pub offered_at_ms: u64,
}

/// The pending offers, and why offers are blocked when they are (then an empty list means
/// "blocked", not "nothing offered").
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OffersDto {
    /// Pending offers, as the daemon lists them.
    pub offers: Vec<OfferDto>,
    /// Empty unless offers are blocked.
    pub problem: String,
}

/// See [`workspace_offers`].
pub async fn offers_with(client: &mut DaemonClient) -> Result<OffersDto, String> {
    let resp = client
        .workspace_pending_offers()
        .await
        .map_err(|e| e.to_string())?;
    Ok(OffersDto {
        offers: resp
            .offers
            .into_iter()
            .map(|o| OfferDto {
                offering_device: o.offering_device,
                workspace_id: o.workspace_id,
                name: o.name,
                offered_at_ms: o.offered_at_ms,
            })
            .collect(),
        problem: resp.offers_problem,
    })
}

/// Workspaces paired devices have offered this device.
#[tracing::instrument(name = "ipc.workspace_offers", skip_all)]
#[tauri::command]
pub async fn workspace_offers(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<OffersDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    offers_with(&mut client).await
}

/// Accepts one offer: the daemon mirrors that workspace into its own folder.
#[tracing::instrument(name = "ipc.accept_offer", skip_all)]
#[tauri::command]
pub async fn accept_offer(
    app: AppHandle,
    state: State<'_, AppState>,
    offering_device: String,
    workspace_id: String,
) -> Result<WorkspaceInfoDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let info = client
        .workspace_accept_offer(offering_device, workspace_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(WorkspaceInfoDto::from(info))
}

/// Declines one offer; false when there was no such offer.
#[tracing::instrument(name = "ipc.decline_offer", skip_all)]
#[tauri::command]
pub async fn decline_offer(
    app: AppHandle,
    state: State<'_, AppState>,
    offering_device: String,
    workspace_id: String,
) -> Result<bool, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    client
        .workspace_decline_offer(offering_device, workspace_id)
        .await
        .map_err(|e| e.to_string())
}
