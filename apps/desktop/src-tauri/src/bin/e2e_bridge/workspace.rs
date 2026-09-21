//! The four workspace-registry bridge commands (task `desktop-workspace-nav-sidebar`), split out
//! of `e2e_bridge.rs` for its own file-length budget — same pattern `commands_workspace.rs` uses
//! to split these out of `commands.rs` for the real Tauri command surface. Same RPC bodies,
//! restated over this bridge's own `DaemonClient`.

use desktop_lib::daemon::DaemonClient;
use desktop_lib::dto::{WorkspaceInfoDto, WorkspaceLayoutDto};
use serde::Deserialize;
use serde_json::Value;

use super::{ApiError, parse};

/// Dispatches one of `list_workspaces`/`add_workspace`/`remove_workspace`/`switch_workspace` —
/// `cmd` is always one of those four, checked by the caller's own match arm.
pub(crate) async fn dispatch_workspace_cmd(
    client: &mut DaemonClient,
    cmd: &str,
    args: Value,
) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    struct RootReq {
        root: String,
    }
    match cmd {
        "list_workspaces" => {
            let workspaces = client.workspace_list().await?;
            let dtos: Vec<WorkspaceInfoDto> =
                workspaces.into_iter().map(WorkspaceInfoDto::from).collect();
            Ok(serde_json::to_value(dtos)?)
        }
        "workspace_layout" => {
            let layout = client.workspace_layout().await?;
            Ok(serde_json::to_value(WorkspaceLayoutDto::from(layout))?)
        }
        "add_workspace" => {
            let r: RootReq = parse(args)?;
            let info = client.workspace_add(std::path::Path::new(&r.root)).await?;
            Ok(serde_json::to_value(WorkspaceInfoDto::from(info))?)
        }
        "remove_workspace" => {
            #[derive(Deserialize)]
            struct Req {
                id: String,
            }
            let r: Req = parse(args)?;
            Ok(Value::Bool(client.workspace_remove(&r.id).await?))
        }
        // Sync, no daemon round trip (`DaemonClient::switch_workspace`'s own signature) — just
        // points this connection's `selector` at `root` for every RPC after this one.
        _ => {
            let r: RootReq = parse(args)?;
            client.switch_workspace(std::path::Path::new(&r.root));
            Ok(Value::Null)
        }
    }
}
