//! The `op_log_all` bridge command (task `desktop-activity-cross-workspace`), split out of
//! `e2e_bridge.rs` for its own file-length budget — same pattern `workspace.rs` uses. Restates
//! `commands_activity.rs::op_log_all_inner`'s body over this bridge's own `DaemonClient`, since
//! that function's own signature is tied to Tauri's `AppHandle`/`State` extractors.

use desktop_lib::daemon::DaemonClient;
use desktop_lib::dto::AggregatedOpLogEntryDto;
use serde_json::Value;
use txtodo_proto::v1 as pb;

use super::ApiError;

/// Same bound `commands_activity.rs::OP_LOG_ALL_CAP` uses.
const OP_LOG_ALL_CAP: usize = 200;

pub(crate) async fn cmd_op_log_all(client: &mut DaemonClient) -> Result<Value, ApiError> {
    let workspaces = client.workspace_list().await?;
    let mut merged = Vec::new();
    for ws in workspaces {
        if !ws.root_exists {
            continue;
        }
        let selector = pb::WorkspaceSelector {
            selector: Some(pb::workspace_selector::Selector::WorkspaceId(
                ws.workspace_id.clone(),
            )),
        };
        let Ok(entries) = client.op_log_for(selector).await else {
            continue;
        };
        merged.extend(entries.into_iter().map(|e| AggregatedOpLogEntryDto {
            principal: e.principal,
            op: e.op,
            at_ms: e.at_ms,
            workspace_id: ws.workspace_id.clone(),
            workspace_root: ws.root.clone(),
        }));
    }
    merged.sort_by_key(|e: &AggregatedOpLogEntryDto| std::cmp::Reverse(e.at_ms));
    merged.truncate(OP_LOG_ALL_CAP);
    Ok(serde_json::to_value(merged)?)
}
