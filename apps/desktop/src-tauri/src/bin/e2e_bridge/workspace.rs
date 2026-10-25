//! The workspace-registry bridge commands (task `desktop-workspace-nav-sidebar`), split out
//! of `e2e_bridge.rs` for its own file-length budget — same pattern `commands_workspace.rs` uses
//! to split these out of `commands.rs` for the real Tauri command surface. Same RPC bodies,
//! restated over this bridge's own `DaemonClient`.

use desktop_lib::daemon::DaemonClient;
use desktop_lib::dto::{WorkspaceInfoDto, WorkspaceLayoutDto};
use serde::Deserialize;
use serde_json::Value;

use super::{ApiError, parse};

/// Dispatches one of `WORKSPACE_CMDS` (list, layout, add, remove, rename, switch) —
/// `cmd` is always one of those, checked by the caller.
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
        "rename_workspace" => {
            #[derive(Deserialize)]
            struct Req {
                id: String,
                name: String,
            }
            let r: Req = parse(args)?;
            let info = client.workspace_rename(&r.id, &r.name).await?;
            Ok(serde_json::to_value(WorkspaceInfoDto::from(info))?)
        }
        "new_list" | "rename_list" | "remove_list" => dispatch_list_cmd(client, cmd, args).await,
        // Sync, no daemon round trip (`DaemonClient::switch_workspace`'s own signature) — just
        // points this connection's `selector` at `root` for every RPC after this one.
        _ => {
            let r: RootReq = parse(args)?;
            client.switch_workspace(std::path::Path::new(&r.root));
            Ok(Value::Null)
        }
    }
}

/// `new_list`, `rename_list`, `remove_list` (task two-kinds-of-workspace): the named-list calls,
/// the same bodies as `desktop_lib::commands_workspace`. Split out of `dispatch_workspace_cmd` for
/// its complexity budget.
async fn dispatch_list_cmd(
    client: &mut DaemonClient,
    cmd: &str,
    args: Value,
) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    struct Req {
        #[serde(default)]
        list: String,
        #[serde(default)]
        name: String,
    }
    let r: Req = parse(args)?;
    match cmd {
        "new_list" => {
            let made = client.list_ensure(&r.name).await?;
            let info = made
                .workspace
                .ok_or_else(|| ApiError("no list made".to_owned()))?;
            Ok(serde_json::to_value(WorkspaceInfoDto::from(info))?)
        }
        "rename_list" => {
            let info = client.list_rename(&r.list, &r.name).await?;
            Ok(serde_json::to_value(WorkspaceInfoDto::from(info))?)
        }
        _ => Ok(Value::String(client.list_remove(&r.list).await?.moved_to)),
    }
}
