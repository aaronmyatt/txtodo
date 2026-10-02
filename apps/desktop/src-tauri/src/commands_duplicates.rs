//! `list_duplicates`: the open file's duplicate groups (ADR 0032). A command of its own, not a
//! wider `list_conflicts` reply, so every existing `list_conflicts` caller keeps its shape.

use crate::commands::ensure_connected;
use crate::dto_duplicates::{DuplicateGroupDto, groups_of};
use crate::state::AppState;
use tauri::{AppHandle, State};

/// Duplicate groups for one workspace-relative document, from the daemon's `ListConflicts`.
/// Ref: https://v2.tauri.app/develop/calling-rust/
#[tracing::instrument(name = "ipc.list_duplicates", skip_all)]
#[tauri::command]
pub async fn list_duplicates(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<DuplicateGroupDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let resp = client
        .list_conflicts(&path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(groups_of(resp))
}
