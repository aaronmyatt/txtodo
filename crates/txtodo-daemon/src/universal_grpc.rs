//! `UniversalTasks` (task `tui-revamp/universal-rpc`, 2026-09-25): every root-list task line
//! across every ready workspace, for the Universal screen, in one call. Replaces desktop's
//! per-workspace `GetFile` loop (`apps/desktop/src-tauri/src/commands_universal.rs`) and gives the
//! TUI the same rows. Device-level like `WorkspaceList`: it walks the registry in its own order and
//! skips a workspace that is still opening, missing or failing instead of waiting or failing.
//! Root lists only; the raw `due:` value goes out as is and each client buckets it against its
//! own local today (ADR 0011, `txtodo_core::universal`).

use std::path::Path;

use tonic::{Request, Response, Status};
use txtodo_core::{LineKind, Task};
use txtodo_model::{FilePath, WorkspaceLayout};
use txtodo_proto::v1 as pb;

use crate::convert::status_of;
use crate::global_service::{GlobalService, workspace_info};
use crate::server::{SharedWorkspace, TxtodoService};

/// The handler `GlobalService` delegates to.
pub(crate) async fn universal_tasks(
    svc: &GlobalService,
    r: Request<pb::UniversalTasksRequest>,
) -> Result<Response<pb::UniversalTasksResponse>, Status> {
    let include_done = r.get_ref().include_done;
    let mut tasks = Vec::new();
    for entry in svc.catalog().list_registered_entries()? {
        let Some(ws) = svc.catalog().ready(entry.id) else {
            continue;
        };
        let info = workspace_info(svc.catalog(), entry);
        match rows_for(ws, &info, include_done).await {
            Ok(rows) => tasks.extend(rows),
            Err(e) => log_skipped(&info.workspace_id, &e),
        }
    }
    Ok(Response::new(pb::UniversalTasksResponse { tasks }))
}

fn log_skipped(workspace: &str, e: &Status) {
    tracing::warn!(workspace, error = %e.message(), "universal_tasks_workspace_skipped");
}

/// The name a client shows: `default` for the default workspace, else the root's folder name.
fn workspace_name(info: &pb::WorkspaceInfo) -> String {
    if info.is_default {
        return String::from("default");
    }
    Path::new(&info.root)
        .file_name()
        .map_or_else(|| info.root.clone(), |n| n.to_string_lossy().into_owned())
}

/// Every task line of one workspace's root list, as rows.
async fn rows_for(
    ws: SharedWorkspace,
    info: &pb::WorkspaceInfo,
    include_done: bool,
) -> Result<Vec<pb::UniversalTask>, Status> {
    let svc = TxtodoService::new(ws);
    let layout = svc.workspace().layout().get();
    let root_list = layout.root_list();
    let contents = svc
        .actor_by_path(&root_list)?
        .get()
        .await
        .map_err(status_of)?;
    let ids = contents.task_id_texts();
    let file = txtodo_core::parse_file(&contents.bytes);
    let mut rows = Vec::new();
    for (i, line) in file.lines.iter().enumerate() {
        let Some(parsed) = line.parse() else {
            continue;
        };
        let LineKind::Task(task) = parsed.kind else {
            continue;
        };
        if task.completed && !include_done {
            continue;
        }
        let mut row = base_row(&task, parsed.raw);
        row.workspace_id.clone_from(&info.workspace_id);
        row.workspace_name = workspace_name(info);
        row.root_list = root_list.to_string();
        row.line_number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        row.task_id = ids.get(i).cloned().unwrap_or_default();
        if let Some(slug) = task.ref_slug() {
            fill_ref(&svc, &layout, (&root_list, slug), &mut row).await;
        }
        rows.push(row);
    }
    Ok(rows)
}

/// The fields one parsed task line gives on its own.
fn base_row(task: &Task<'_>, raw: &str) -> pb::UniversalTask {
    let priority = task
        .priority
        .map(|p| p.as_char())
        .or_else(|| task.tag("pri").and_then(|v| v.chars().next()));
    pb::UniversalTask {
        raw: raw.to_owned(),
        done: task.completed,
        completion_date: task
            .completion_date
            .map(|d| d.to_string())
            .unwrap_or_default(),
        priority: priority.map(String::from).unwrap_or_default(),
        due: task.tag("due").unwrap_or_default().to_owned(),
        projects: task.projects().map(str::to_owned).collect(),
        contexts: task.contexts().map(str::to_owned).collect(),
        ..pb::UniversalTask::default()
    }
}

/// `has_ref`, the sub-list's progress when the daemon holds one, and `has_notes`.
async fn fill_ref(
    svc: &TxtodoService,
    layout: &WorkspaceLayout,
    (owner, slug): (&FilePath, &str),
    row: &mut pb::UniversalTask,
) {
    row.has_ref = true;
    let dir = layout.ref_dir_for(owner, slug);
    if let Ok(sub) = svc.actor(&format!("{dir}/todo.txt")) {
        row.ref_progress = svc.progress_for(&sub).await.ok();
    }
    let root = svc.workspace().root().to_path_buf();
    row.has_notes = root.join(&dir).join("notes.md").is_file();
}
