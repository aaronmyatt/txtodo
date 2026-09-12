//! Activity-feed command (ADR 0004 `oplog.db`, plan M7): `OpLogStream`. Split out of
//! `commands.rs` the way `crates/txtodo-daemon/src/server.rs` delegates to `activity.rs`; same
//! `ensure_connected` + lock + map-err-to-String pattern as every command there.

use crate::commands::ensure_connected;
use crate::dto::OpLogEntryDto;
use crate::state::AppState;
use tauri::{AppHandle, State};

/// Newest ops across every tracked file, at most 200, newest first: one bounded read, not a live
/// tail — unlike [`crate::commands::watch`], this returns the whole page at once instead of
/// pushing `daemon-change` events.
#[tauri::command]
pub async fn op_log(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<OpLogEntryDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let entries = client.op_log().await.map_err(|e| e.to_string())?;
    Ok(entries.into_iter().map(OpLogEntryDto::from).collect())
}
