//! Paired-device commands for Settings > Devices (task desktop-ui-revamp): `DeviceList` and
//! `DeviceRemove`, the same RPCs `txtodo device list/remove` use. Removal rotates the group key;
//! the frontend asks first and shows the daemon's caveat.

use crate::commands::ensure_connected;
use crate::dto_devices::{DeviceDto, DeviceRemoveDto};
use crate::state::AppState;
use tauri::{AppHandle, State};

/// Every paired device of the open workspace's sync group, removed ones included.
#[tracing::instrument(name = "ipc.device_list", skip_all)]
#[tauri::command]
pub async fn device_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<DeviceDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let resp = client.device_list().await.map_err(|e| e.to_string())?;
    Ok(resp.devices.into_iter().map(DeviceDto::from).collect())
}

/// Removes one device by id and rotates the group key to the rest.
#[tracing::instrument(name = "ipc.device_remove", skip_all)]
#[tauri::command]
pub async fn device_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<DeviceRemoveDto, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let resp = client.device_remove(id).await.map_err(|e| e.to_string())?;
    Ok(DeviceRemoveDto::from(resp))
}
