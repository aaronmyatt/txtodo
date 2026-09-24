//! The Universal screen's daemon calls (task `tui-revamp/tui-universal`): the rows from
//! `UniversalTasks`; `x` completes a row in its own workspace (the selector points there for the
//! one call) with an Undo toast; Enter switches to the row's workspace and puts the cursor on its
//! line. The rows refresh on entering the screen, on the 1 s tick while it shows, and after each
//! change, since the loop's `Watch` covers only the open workspace.

use std::time::Instant;

use txtodo_proto::v1 as pb;

use crate::daemon::{Daemon, DaemonError};
use crate::daemon_workspace::workspace_id_selector;
use crate::state::AppState;
use crate::state_nav::Screen;
use crate::state_universal::UTask;

/// Re-reads every workspace's tasks, done ones included (the screen filters them); a failed read
/// keeps the old rows.
pub async fn refresh(daemon: &mut Daemon, state: &mut AppState) {
    if let Ok(reply) = daemon.universal_tasks(true).await {
        state.universal.tasks = reply.tasks.into_iter().map(utask).collect();
    }
}

fn utask(t: pb::UniversalTask) -> UTask {
    UTask {
        workspace_id: t.workspace_id,
        workspace: t.workspace_name,
        root_list: t.root_list,
        line_number: t.line_number,
        task_id: t.task_id,
        done: t.done,
        priority: t.priority.chars().next(),
        due: (!t.due.is_empty()).then_some(t.due),
        projects: t.projects,
        contexts: t.contexts,
        progress: t
            .ref_progress
            .filter(|p| p.total > 0)
            .map(|p| (p.done, p.total)),
        has_notes: t.has_notes,
        raw: t.raw,
    }
}

/// `Complete` for an open task, `Reopen` for a done one (the daemon's `Complete` leaves a done line
/// alone).
fn toggle(task: &UTask) -> pb::Mutation {
    let target = Some(pb::TaskRef {
        line_number: task.line_number,
        task_id: task.task_id.clone(),
    });
    let kind = if task.done {
        pb::mutation::Kind::Reopen(pb::Reopen { task: target })
    } else {
        pb::mutation::Kind::Complete(pb::Complete {
            task: target,
            today: crate::commands::today_local(),
        })
    };
    pb::Mutation { kind: Some(kind) }
}

/// `x`: completes (or reopens) `task` in its workspace, toasts with Undo, and re-reads the rows.
pub async fn complete(
    daemon: &mut Daemon,
    state: &mut AppState,
    task: UTask,
) -> Result<(), DaemonError> {
    let req = pb::ApplyRequest {
        path: task.root_list.clone(),
        mutations: vec![toggle(&task)],
        source: "tui".to_owned(),
        ..pb::ApplyRequest::default()
    };
    let open = daemon.selector_for_restore();
    daemon.set_selector(Some(workspace_id_selector(&task.workspace_id)));
    let reply = daemon.apply(req).await;
    daemon.set_selector(open);
    match reply {
        Ok(reply) => {
            let change =
                state
                    .shell
                    .record_in(&task.root_list, reply.applied, Some(task.workspace_id));
            let word = if task.done { "Reopened" } else { "Completed" };
            let message = format!("{word} in {}", task.workspace);
            state.shell.toast(message, Some(change), Instant::now());
        }
        Err(DaemonError::Rpc(status)) => state.last_error = Some(status.message().to_owned()),
        Err(e) => return Err(e),
    }
    refresh(daemon, state).await;
    Ok(())
}

/// Enter: points the TUI at the row's workspace (the open one too: switching re-reads it), goes to
/// Tasks, and puts the cursor on the row's line.
pub async fn open(
    daemon: &mut Daemon,
    state: &mut AppState,
    task: UTask,
) -> Result<(), DaemonError> {
    crate::app_workspace::switch_workspace(daemon, state, &task.workspace_id).await?;
    if state.last_error.is_some() {
        return Ok(());
    }
    state.nav.screen = Screen::Tasks;
    let row = state
        .lines
        .iter()
        .position(|l| !task.task_id.is_empty() && l.task_ref_id() == task.task_id)
        .or_else(|| {
            state
                .lines
                .iter()
                .position(|l| l.line_number == task.line_number)
        });
    if let Some(row) = row {
        state.cursor = row;
    }
    Ok(())
}
