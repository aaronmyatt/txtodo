//! The cross-workspace bridge commands: `op_log_all` (body in `activity.rs`) and the Universal
//! ones (task desktop-ui-revamp), `universal_tasks`, `universal_set_done` and `universal_undo`.
//! The Universal ones do not restate their bodies: they call the same `desktop_lib` functions
//! the Tauri commands do, so a Playwright spec exercises the shipped code against a real daemon.

use desktop_lib::commands_universal::universal_rows;
use desktop_lib::commands_universal_edit::{RowRef, set_done, undo, undo_change_with};
use desktop_lib::daemon::DaemonClient;
use serde::Deserialize;
use serde_json::Value;

use super::activity::cmd_op_log_all;
use super::{ApiError, parse};

/// The commands this module answers; the caller checks membership first.
pub(crate) const CROSS_WORKSPACE_CMDS: [&str; 5] = [
    "op_log_all",
    "universal_tasks",
    "universal_set_done",
    "universal_undo",
    "undo_change",
];

/// Args arrive in the frontend's camelCase, as Tauri maps them:
/// https://v2.tauri.app/develop/calling-rust/#passing-arguments
pub(crate) async fn dispatch(
    client: &mut DaemonClient,
    cmd: &str,
    args: Value,
) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TasksReq {
        include_done: Option<bool>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SetDoneReq {
        workspace_id: String,
        path: String,
        line_number: u32,
        task_id: String,
        done: bool,
        today: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UndoReq {
        workspace_id: String,
        path: String,
        steps: Option<u32>,
    }
    match cmd {
        "op_log_all" => cmd_op_log_all(client).await,
        "undo_change" => cmd_undo_change(client, args).await,
        "universal_tasks" => {
            let r: TasksReq = parse(args)?;
            let rows = universal_rows(client, r.include_done.unwrap_or(false))
                .await
                .map_err(ApiError)?;
            Ok(serde_json::to_value(rows)?)
        }
        "universal_set_done" => {
            let r: SetDoneReq = parse(args)?;
            let row = RowRef {
                workspace_id: r.workspace_id,
                path: r.path,
                line_number: r.line_number,
                task_id: r.task_id,
            };
            let res = set_done(client, row, r.done, r.today)
                .await
                .map_err(ApiError)?;
            Ok(serde_json::to_value(res)?)
        }
        _ => {
            let r: UndoReq = parse(args)?;
            let res = undo(client, r.workspace_id, &r.path, r.steps.unwrap_or(1))
                .await
                .map_err(ApiError)?;
            Ok(serde_json::to_value(res)?)
        }
    }
}

/// `undo_change`: one of the session's changes taken back (`u`, a toast's Undo).
async fn cmd_undo_change(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UndoChangeReq {
        path: String,
        steps: u32,
        workspace_root: Option<String>,
    }
    let r: UndoChangeReq = parse(args)?;
    let root = r.workspace_root.unwrap_or_default();
    let res = undo_change_with(client, &r.path, r.steps, &root)
        .await
        .map_err(ApiError)?;
    Ok(serde_json::to_value(res)?)
}
