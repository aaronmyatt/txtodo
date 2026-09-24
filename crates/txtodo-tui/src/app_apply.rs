//! Sending an edit and undoing one (tasks `tui-revamp/tui-shell`): `app::perform`'s `Apply` and
//! `Undo` arms, split out for its complexity budget. A refusal is the daemon doing its job: it goes
//! on the status line (and, for a typed line, the refused-edit banner), never out of the loop. A
//! transport error still does.

use std::time::Instant;

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};
use crate::state::AppState;
use crate::state_shell::Refused;

/// Sends `req`, then re-baselines from the daemon. On success it clears the last refusal, marks
/// the footer "saved" and shows the toast the command asked for, with Undo.
pub(crate) async fn apply(
    daemon: &mut Daemon,
    state: &mut AppState,
    req: pb::ApplyRequest,
) -> Result<(), DaemonError> {
    let path = req.path.clone();
    let typed = typed_line(&req);
    crate::app_detail::ensure_sub_list(daemon, state, &path).await?;
    match daemon.apply(req).await {
        Ok(reply) => landed(state, &path, reply.applied),
        Err(DaemonError::Rpc(status)) => {
            refused(state, status.message(), typed);
            return Ok(());
        }
        Err(e) => return Err(e),
    }
    crate::app_detail::refetch(daemon, state, &path).await
}

fn landed(state: &mut AppState, path: &str, ops: u32) {
    let now = Instant::now();
    state.last_error = None;
    state.shell.refused = None;
    state.shell.saved_at = Some(now);
    let change = state.shell.record(path, ops);
    if let Some(message) = state.shell.pending_toast.take() {
        state.shell.toast(message, Some(change), now);
    }
}

fn refused(state: &mut AppState, error: &str, typed: Option<String>) {
    state.shell.pending_toast = None;
    state.last_error = Some(error.to_owned());
    state.shell.refused = typed.map(|text| Refused {
        error: error.to_owned(),
        text,
    });
}

/// The line a request typed (an `Add` or an `Edit`), kept so a refused one can be copied back.
fn typed_line(req: &pb::ApplyRequest) -> Option<String> {
    match req.mutations.first()?.kind.as_ref()? {
        pb::mutation::Kind::Add(add) => Some(add.line.clone()),
        pb::mutation::Kind::Edit(edit) => Some(edit.new_line.clone()),
        _ => None,
    }
}

/// `u` or a toast's Undo: the daemon reverts the newest `steps` ops to `path` (one change's
/// worth) in `workspace` (the open one when `None`), then the list re-baselines. A refusal
/// (nothing left to undo) goes on the status line.
pub(crate) async fn undo(
    daemon: &mut Daemon,
    state: &mut AppState,
    path: &str,
    steps: u32,
    workspace: Option<&str>,
) -> Result<(), DaemonError> {
    let reply = match workspace {
        Some(id) => {
            let open = daemon.selector_for_restore();
            daemon.set_selector(Some(crate::daemon_workspace::workspace_id_selector(id)));
            let reply = daemon.undo(path, steps).await;
            daemon.set_selector(open);
            reply
        }
        None => daemon.undo(path, steps).await,
    };
    match reply {
        Ok(_) => {}
        Err(DaemonError::Rpc(status)) => {
            state.last_error = Some(status.message().to_owned());
            return Ok(());
        }
        Err(e) => return Err(e),
    }
    if workspace.is_none() {
        crate::app_detail::refetch(daemon, state, path).await?;
    } else {
        crate::app_universal::refresh(daemon, state).await;
    }
    state.shell.toast("Undone", None, Instant::now());
    Ok(())
}
