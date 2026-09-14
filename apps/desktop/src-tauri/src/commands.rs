//! Thin Tauri commands (design §7: "all clients are thin ... none of them parse the file
//! themselves"). Every command locks the shared client, makes one RPC, and converts the
//! response to a serde DTO; the frontend addresses documents by workspace-relative path only,
//! never a raw OS path. Ref: <https://v2.tauri.app/develop/calling-rust/>

use crate::daemon::{self, DaemonClient, DaemonError};
use crate::dto::{
    ApplyResultDto, ChangeDto, FileContentsDto, FileInfoDto, HistoryDto, MutationDto,
    ResolutionDto, ReviewFlagDto, TaskRefDto,
};
use crate::state::AppState;
use crate::status::DaemonStatus;
use tauri::{AppHandle, Emitter, State};
use txtodo_proto::v1 as pb;

/// Updates the shared status and mirrors it to the frontend as a `daemon-status` event.
async fn set_status(app: &AppHandle, state: &AppState, status: DaemonStatus) {
    *state.status.lock().await = status;
    let _ = app.emit("daemon-status", status);
}

/// Spawns/dials the daemon and stores the connected client, narrating the attempt through
/// `daemon-status` events. Never panics: failures come back as a `DaemonError` and land on
/// `DaemonStatus::Dead` in the caller. `pub(crate)` so `lib.rs` can kick off the first connect
/// from `setup` without going through the command-invoke machinery.
pub(crate) async fn connect_and_store(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), DaemonError> {
    set_status(app, state, DaemonStatus::Spawning).await;
    let sock = daemon::ensure_daemon(&state.config).await?;
    set_status(app, state, DaemonStatus::Connecting).await;
    let mut client = DaemonClient::connect(&sock).await?;
    client.wait_until_ready().await?;
    *state.client.lock().await = Some(client);
    set_status(app, state, DaemonStatus::Connected).await;
    Ok(())
}

/// Ensures a client is stored, connecting/spawning first if this is the first call. `pub(crate)`
/// (not private) so the sibling `commands_*` modules (split out of this file the way
/// `crates/txtodo-daemon/src/server.rs` splits into `notes.rs`/`tokens.rs`/`pairing_grpc.rs`/
/// `activity.rs`) can reach it — Rust's default privacy does not extend to sibling modules, only
/// descendants.
pub(crate) async fn ensure_connected(app: &AppHandle, state: &AppState) -> Result<(), String> {
    if state.client.lock().await.is_some() {
        return Ok(());
    }
    connect_and_store(app, state).await.map_err(|e| {
        e.to_string() // recorded by connect_and_store's own Dead transition below
    })
}

/// Current connectivity state; also pushed as a `daemon-status` event on every change. Queried
/// once on startup so the frontend has a value before the first event arrives.
#[tauri::command]
pub async fn daemon_status(state: State<'_, AppState>) -> Result<DaemonStatus, String> {
    Ok(*state.status.lock().await)
}

/// Absolute workspace root, for the detail view's footer (tasks/desktop-detail-view/notes.md:
/// "footer: absolute directory path + sync status") — display only. The frontend never uses this
/// to open a file itself (design §7: every read/write still crosses the `DaemonClient`); it only
/// joins this with a workspace-relative directory to show the human where they are on disk.
#[tauri::command]
pub fn workspace_root(state: State<'_, AppState>) -> String {
    state.config.workspace.display().to_string()
}

/// Records whether the main window's edit popover currently has an unsaved edit; the quick-add
/// global hotkey's handler reads this to decide whether to open quick-add or refocus the main
/// window instead (tasks/desktop-quick-add/notes.md).
#[tauri::command]
pub fn set_main_popover_dirty(state: State<'_, AppState>, dirty: bool) {
    state
        .main_popover_dirty
        .store(dirty, std::sync::atomic::Ordering::Relaxed);
}

/// Retries the connect/spawn sequence; wired to the reconnect banner's retry button.
#[tauri::command]
pub async fn retry_connect(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DaemonStatus, String> {
    if connect_and_store(&app, &state).await.is_err() {
        set_status(&app, &state, DaemonStatus::Dead).await;
    }
    Ok(*state.status.lock().await)
}

/// Every synced document with its current projection hash.
#[tauri::command]
pub async fn list_files(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<FileInfoDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let resp = client.list_files().await.map_err(|e| e.to_string())?;
    Ok(resp.files.into_iter().map(FileInfoDto::from).collect())
}

/// The exact bytes the daemon holds for one workspace-relative document path.
#[tauri::command]
pub async fn get_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<FileContentsDto, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let resp = client.get_file(&path).await.map_err(|e| e.to_string())?;
    Ok(FileContentsDto::from(resp))
}

/// Starts a `Watch` stream for `paths` (every document when empty) and forwards each `Change`
/// as a `daemon-change` event; returns once the stream is established, not when it ends.
#[tauri::command]
pub async fn watch(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<(), String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let mut stream = client.watch(paths).await.map_err(|e| e.to_string())?;
    drop(guard);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Ok(Some(change)) = stream.message().await {
            let _ = app.emit("daemon-change", ChangeDto::from(change));
        }
    });
    Ok(())
}

/// Intent-level mutations on one workspace-relative document; the daemon turns them into ops.
#[tauri::command]
pub async fn apply(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    mutations: Vec<MutationDto>,
) -> Result<ApplyResultDto, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let req = pb::ApplyRequest {
        path,
        mutations: mutations.into_iter().map(pb::Mutation::from).collect(),
        agent: None, // unset = the user on this device
        workspace: None,
    };
    let resp = client.apply(req).await.map_err(|e| e.to_string())?;
    Ok(ApplyResultDto::from(resp))
}

/// Ops newest first, filtered by path and/or task (empty = every document/task).
#[tauri::command]
pub async fn history(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    task_id: String,
    limit: u32,
) -> Result<HistoryDto, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let req = pb::HistoryRequest {
        path,
        task_id,
        limit,
        before_seq: 0,
        workspace: None,
    };
    let resp = client.history(req).await.map_err(|e| e.to_string())?;
    Ok(HistoryDto::from(resp))
}

/// Resolves one `needs_review` flag; maps to the daemon's `ResolveConflict` RPC.
#[tauri::command]
pub async fn resolve(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    task: TaskRefDto,
    resolution: ResolutionDto,
) -> Result<ApplyResultDto, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let req = pb::ResolveRequest {
        path,
        task: Some(task.into()),
        resolution: pb::Resolution::from(resolution) as i32,
        workspace: None,
    };
    let resp = client.resolve(req).await.map_err(|e| e.to_string())?;
    Ok(ApplyResultDto::from(resp))
}

/// Open `needs_review` flags for one workspace-relative document (plan M4): two devices rewrote
/// the same word.
#[tauri::command]
pub async fn list_conflicts(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<ReviewFlagDto>, String> {
    ensure_connected(&app, &state).await?;
    let mut guard = state.client.lock().await;
    let client = guard.as_mut().ok_or("daemon not connected")?;
    let resp = client
        .list_conflicts(&path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(resp.flags.into_iter().map(ReviewFlagDto::from).collect())
}
