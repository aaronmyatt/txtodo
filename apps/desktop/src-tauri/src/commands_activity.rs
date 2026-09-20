//! Activity-feed commands (ADR 0004 `oplog.db`, plan M7; cross-workspace fan-out, task
//! `desktop-activity-cross-workspace`): `OpLogStream`. Split out of `commands.rs` the way
//! `crates/txtodo-daemon/src/server.rs` delegates to `activity.rs`; same `ensure_connected` + lock
//! + map-err-to-String pattern as every command there.

use crate::commands::ensure_connected;
use crate::daemon::DaemonClient;
use crate::dto::{AggregatedOpLogEntryDto, OpLogEntryDto, is_ready_or_unknown};
use crate::state::AppState;
use tauri::{AppHandle, State};
use txtodo_proto::v1 as pb;

/// Newest ops across every tracked file, at most 200, newest first: one bounded read, not a live
/// tail — unlike [`crate::commands::watch`], this returns the whole page at once instead of
/// pushing `daemon-change` events.
#[tracing::instrument(name = "ipc.op_log", skip_all)]
#[tauri::command]
pub async fn op_log(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<OpLogEntryDto>, String> {
    op_log_inner(app, state).await
}

async fn op_log_inner(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<OpLogEntryDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let entries = client.op_log().await.map_err(|e| e.to_string())?;
    Ok(entries.into_iter().map(OpLogEntryDto::from).collect())
}

/// Same 200-entry, newest-first bound as [`op_log`], but merged across every *registered*
/// workspace (`WorkspaceList`; `WorkspaceCatalog::resolve` opens each on demand as its selector is
/// used, per that RPC's own doc — no daemon-side "currently open" list needed). A workspace whose
/// root no longer exists, or whose fetch fails, is skipped rather than failing the whole call
/// (mirrors `WorkspaceCatalog::open_all_registered`'s own "one bad workspace never blanks the
/// rest" rule).
#[tracing::instrument(name = "ipc.op_log_all", skip_all)]
#[tauri::command]
pub async fn op_log_all(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<AggregatedOpLogEntryDto>, String> {
    op_log_all_inner(app, state).await
}

/// Same bound `op_log`'s own daemon-side stream caps at.
const OP_LOG_ALL_CAP: usize = 200;

async fn op_log_all_inner(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<AggregatedOpLogEntryDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut client = state.client_snapshot().await?;
    let workspaces = client.workspace_list().await.map_err(|e| e.to_string())?;

    let mut merged = Vec::new();
    for ws in workspaces {
        // Only a workspace the daemon has finished opening: a fan-out must not promote every
        // workspace (see `is_ready_or_unknown`).
        if ws.root_exists && is_ready_or_unknown(&ws) {
            merged.extend(op_log_one_workspace(&mut client, ws).await);
        }
    }
    merged.sort_by_key(|e: &AggregatedOpLogEntryDto| std::cmp::Reverse(e.at_ms));
    merged.truncate(OP_LOG_ALL_CAP);
    Ok(merged)
}

/// One workspace's tagged entries, or empty on failure — logged, never propagated (module doc).
async fn op_log_one_workspace(
    client: &mut DaemonClient,
    ws: pb::WorkspaceInfo,
) -> Vec<AggregatedOpLogEntryDto> {
    let selector = pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::WorkspaceId(
            ws.workspace_id.clone(),
        )),
    };
    let Ok(entries) = client.op_log_for(selector).await else {
        tracing::debug!(workspace = %ws.workspace_id, "op_log_all_workspace_failed");
        return Vec::new();
    };
    entries
        .into_iter()
        .map(|e| AggregatedOpLogEntryDto {
            principal: e.principal,
            op: e.op,
            at_ms: e.at_ms,
            workspace_id: ws.workspace_id.clone(),
            workspace_root: ws.root.clone(),
        })
        .collect()
}
