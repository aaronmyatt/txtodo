//! The `ref_dir` bridge command (task desktop-sublist-start) and the dispatcher for the three
//! `commands_notes.rs`-shaped commands, split out of `e2e_bridge.rs` for its file-length and
//! complexity budgets — same RPC bodies, restated over this bridge's own `DaemonClient`.

use desktop_lib::daemon::DaemonClient;
use desktop_lib::dto::{RefDirInfoDto, TaskRefDto};
use serde::Deserialize;
use serde_json::Value;

use super::{ApiError, parse};

/// `cmd` is one of `get_notes`/`edit_notes`/`ref_dir`, checked by the caller's own match arm.
pub(crate) async fn dispatch_notes_cmd(
    client: &mut DaemonClient,
    cmd: &str,
    args: Value,
) -> Result<Value, ApiError> {
    match cmd {
        "get_notes" => super::cmd_get_notes(client, args).await,
        "edit_notes" => super::cmd_edit_notes(client, args).await,
        _ => cmd_ref_dir(client, args).await,
    }
}

async fn cmd_ref_dir(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    struct Req {
        path: String,
        task: TaskRefDto,
        ensure: bool,
    }
    let r: Req = parse(args)?;
    let resp = client.ref_dir(&r.path, r.task.into(), r.ensure).await?;
    Ok(serde_json::to_value(RefDirInfoDto::from(resp))?)
}
