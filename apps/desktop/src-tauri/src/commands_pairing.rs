//! Pairing commands (plan M4, design §4): `PairOffer`/`PairAccept`/`PairConfirmSas`. Split out of
//! `commands.rs` the way `crates/txtodo-daemon/src/server.rs` delegates to `pairing_grpc.rs`;
//! same `ensure_connected` + lock + map-err-to-String pattern as every command there.

use crate::commands::ensure_connected;
use crate::dto::{PairOfferDto, PairResultDto};
use crate::state::AppState;
use tauri::{AppHandle, State};

/// Starts a pairing handshake on this device and returns the QR payload: identity + handshake
/// material only, never the group key or a private key.
#[tracing::instrument(name = "ipc.pair_offer", skip_all)]
#[tauri::command]
pub async fn pair_offer(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PairOfferDto, String> {
    pair_offer_inner(app, state).await
}

async fn pair_offer_inner(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PairOfferDto, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let resp = client.pair_offer().await.map_err(|e| e.to_string())?;
    Ok(PairOfferDto::from(resp))
}

/// Accepts a peer's scanned `PairOffer` (`code`) and begins the X25519 handshake; returns the
/// 6-word SAS to show the human.
#[tracing::instrument(name = "ipc.pair_accept", skip_all)]
#[tauri::command]
pub async fn pair_accept(
    app: AppHandle,
    state: State<'_, AppState>,
    code: String,
) -> Result<PairResultDto, String> {
    pair_accept_inner(app, state, code).await
}

async fn pair_accept_inner(
    app: AppHandle,
    state: State<'_, AppState>,
    code: String,
) -> Result<PairResultDto, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let resp = client.pair_accept(code).await.map_err(|e| e.to_string())?;
    Ok(PairResultDto::from(resp))
}

/// Confirms the SAS shown to the human on this device. The group key lands only once both sides
/// have confirmed.
#[tracing::instrument(name = "ipc.pair_confirm_sas", skip_all)]
#[tauri::command]
pub async fn pair_confirm_sas(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PairResultDto, String> {
    pair_confirm_sas_inner(app, state).await
}

async fn pair_confirm_sas_inner(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PairResultDto, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let resp = client.pair_confirm_sas().await.map_err(|e| e.to_string())?;
    Ok(PairResultDto::from(resp))
}
