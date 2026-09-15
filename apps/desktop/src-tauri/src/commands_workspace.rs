//! Workspace-registry Tauri commands (ADR 0025, task `desktop-workspace-switcher`), split out of
//! `commands.rs` the way `commands_notes.rs`/`commands_pairing.rs`/`commands_tokens.rs`/
//! `commands_activity.rs` already are.

use crate::commands::ensure_connected;
use crate::dto::WorkspaceInfoDto;
use crate::state::AppState;
use std::path::PathBuf;
use tauri::{AppHandle, State};

/// Every registered workspace, oldest first.
#[tauri::command]
pub async fn list_workspaces(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceInfoDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let workspaces = client.workspace_list().await.map_err(|e| e.to_string())?;
    Ok(workspaces.into_iter().map(WorkspaceInfoDto::from).collect())
}

/// Registers `root` (idempotent) without switching to it or opening it.
#[tauri::command]
pub async fn add_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    root: String,
) -> Result<WorkspaceInfoDto, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let info = client
        .workspace_add(&PathBuf::from(root))
        .await
        .map_err(|e| e.to_string())?;
    Ok(WorkspaceInfoDto::from(info))
}

/// Un-registers a workspace id; never touches its `.txtodo/` state on disk. Refuses to remove
/// the currently switched-to workspace — switch away first, the same "never pull the rug out
/// from under yourself" guard `txtodo device remove` applies to this device itself.
#[tauri::command]
pub async fn remove_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    root: String,
) -> Result<bool, String> {
    let current = state.current_workspace.lock().await.clone();
    if current.as_path() == std::path::Path::new(&root) {
        return Err("cannot remove the current workspace; switch away first".to_owned());
    }
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    client
        .workspace_remove(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Points every future command at `root` instead — no reconnect, since the global daemon already
/// serves every registered workspace over the one connection `ensure_connected` maintains.
#[tauri::command]
pub async fn switch_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    root: String,
) -> Result<(), String> {
    ensure_connected(&app, &state).await?;
    let path = PathBuf::from(root);
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    client.switch_workspace(&path);
    drop(guard);
    *state.current_workspace.lock().await = path;
    Ok(())
}
