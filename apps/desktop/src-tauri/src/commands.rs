//! Thin Tauri commands (design §7: "all clients are thin ... none of them parse the file
//! themselves"). Every command locks the shared client, makes one RPC, and converts the
//! response to a serde DTO; the frontend addresses documents by workspace-relative path only,
//! never a raw OS path. Ref: <https://v2.tauri.app/develop/calling-rust/>
//!
//! Every `#[tauri::command]` fn here is a thin wrapper carrying an `ipc.<name>` span
//! (`#[tracing::instrument(skip_all)]`, root todo.txt `logging-desktop`) around a same-named
//! `_inner` doing the real work — `#[instrument]`'s own expansion costs `clippy::cognitive_
//! complexity` points, so the wrapper stays a one-line delegate and the original body (already
//! at or near the budget in a few of these) is untouched in `_inner`. `skip_all` on every one:
//! several of these carry real task/note text as an argument (`apply`'s `mutations`, `resolve`'s
//! `task`), so nothing about a command's arguments is ever captured in the span — ids, counts and
//! outcomes only, never content (CLAUDE.md's logging rule). `#[tracing::instrument]` is listed
//! *above* `#[tauri::command]` here, matching `txtodo-mcp/src/schema.rs`'s `#[tool]` convention,
//! though for a different reason: unlike `rmcp`'s `#[tool]` (which rewrites an `async fn` into a
//! sync fn returning a boxed future, forcing that order), `tauri-macros` 2.6.3's `command::
//! wrapper::wrapper` forwards the annotated `ItemFn` into its output completely unchanged (`quote!
//! (#maybe_allow_unused #function ...)`, `wrapper.rs:311-312`) — it only emits an additional
//! `macro_rules!` beside it for the invoke-handler registration, never touching the function's own
//! body or signature. So either attribute order compiles and spans correctly here (confirmed by
//! compiling both orders against `ui_log` while this task was built); the ordering above is kept
//! only for one consistent house style with the `mcp.call` convention, not because tauri requires
//! it.

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
    let workspace = state.current_workspace.lock().await.clone();
    let selector = Some(pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::Path(
            workspace.display().to_string(),
        )),
    });
    let mut client = DaemonClient::connect(&sock, selector).await?;
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
#[tracing::instrument(name = "ipc.daemon_status", skip_all)]
#[tauri::command]
pub async fn daemon_status(state: State<'_, AppState>) -> Result<DaemonStatus, String> {
    daemon_status_inner(state).await
}

async fn daemon_status_inner(state: State<'_, AppState>) -> Result<DaemonStatus, String> {
    Ok(*state.status.lock().await)
}

/// Absolute workspace root, for the detail view's footer (tasks/desktop-detail-view/notes.md:
/// "footer: absolute directory path + sync status") — display only. The frontend never uses this
/// to open a file itself (design §7: every read/write still crosses the `DaemonClient`); it only
/// joins this with a workspace-relative directory to show the human where they are on disk.
/// Reads `current_workspace` (ADR 0025, task `desktop-workspace-switcher`), not the fixed startup
/// `config.workspace` — `switch_workspace` changes this without restarting the app.
#[tracing::instrument(name = "ipc.workspace_root", skip_all)]
#[tauri::command]
pub async fn workspace_root(state: State<'_, AppState>) -> Result<String, String> {
    workspace_root_inner(state).await
}

async fn workspace_root_inner(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.current_workspace.lock().await.display().to_string())
}

/// Advisory-only (never fails, never blocks): true when no prior `txtodo skill install` has run
/// on this machine yet. See `crate::status::skill_hint_needed`'s doc comment for the mirrored
/// checks this duplicates on purpose.
#[tracing::instrument(name = "ipc.skill_hint", skip_all)]
#[tauri::command]
pub fn skill_hint() -> bool {
    crate::status::skill_hint_needed(crate::status::home_dir().as_deref())
}

/// Records whether the main window's edit popover currently has an unsaved edit; the quick-add
/// global hotkey's handler reads this to decide whether to open quick-add or refocus the main
/// window instead (tasks/desktop-quick-add/notes.md).
#[tracing::instrument(name = "ipc.set_main_popover_dirty", skip_all)]
#[tauri::command]
pub fn set_main_popover_dirty(state: State<'_, AppState>, dirty: bool) {
    set_main_popover_dirty_inner(state, dirty);
}

fn set_main_popover_dirty_inner(state: State<'_, AppState>, dirty: bool) {
    state
        .main_popover_dirty
        .store(dirty, std::sync::atomic::Ordering::Relaxed);
}

/// Retries the connect/spawn sequence; wired to the reconnect banner's retry button.
#[tracing::instrument(name = "ipc.retry_connect", skip_all)]
#[tauri::command]
pub async fn retry_connect(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DaemonStatus, String> {
    retry_connect_inner(app, state).await
}

async fn retry_connect_inner(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DaemonStatus, String> {
    if connect_and_store(&app, &state).await.is_err() {
        set_status(&app, &state, DaemonStatus::Dead).await;
    }
    Ok(*state.status.lock().await)
}

/// Every synced document with its current projection hash.
#[tracing::instrument(name = "ipc.list_files", skip_all)]
#[tauri::command]
pub async fn list_files(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<FileInfoDto>, String> {
    list_files_inner(app, state).await
}

async fn list_files_inner(
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
#[tracing::instrument(name = "ipc.get_file", skip_all)]
#[tauri::command]
pub async fn get_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<FileContentsDto, String> {
    get_file_inner(app, state, path).await
}

async fn get_file_inner(
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
#[tracing::instrument(name = "ipc.watch", skip_all)]
#[tauri::command]
pub async fn watch(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<(), String> {
    watch_inner(app, state, paths).await
}

async fn watch_inner(
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
#[tracing::instrument(name = "ipc.apply", skip_all)]
#[tauri::command]
pub async fn apply(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    mutations: Vec<MutationDto>,
) -> Result<ApplyResultDto, String> {
    apply_inner(app, state, path, mutations).await
}

async fn apply_inner(
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
#[tracing::instrument(name = "ipc.history", skip_all)]
#[tauri::command]
pub async fn history(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    task_id: String,
    limit: u32,
) -> Result<HistoryDto, String> {
    history_inner(app, state, path, task_id, limit).await
}

async fn history_inner(
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
#[tracing::instrument(name = "ipc.resolve", skip_all)]
#[tauri::command]
pub async fn resolve(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    task: TaskRefDto,
    resolution: ResolutionDto,
) -> Result<ApplyResultDto, String> {
    resolve_inner(app, state, path, task, resolution).await
}

async fn resolve_inner(
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
#[tracing::instrument(name = "ipc.list_conflicts", skip_all)]
#[tauri::command]
pub async fn list_conflicts(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<ReviewFlagDto>, String> {
    list_conflicts_inner(app, state, path).await
}

async fn list_conflicts_inner(
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

/// Bridges a frontend log call into this process's own tracing subscriber (root todo.txt
/// `logging-desktop`) so a UI-side event lands in the same `desktop.log.*`/stderr this crate
/// already writes to — the receiving end only; the frontend's own call sites (what's safe to pass
/// as `message`/`fields`) are a separate, later backlog item (`logging-frontend`). `level` is one
/// of `error`/`warn`/`info`/`debug`/`trace` (anything else logs at `info`); `message` and `fields`
/// are exactly what this command exists to carry into the log, so — unlike every other command in
/// this crate — they are deliberately *not* skipped: passing them through *is* the feature, not an
/// accidental content leak (CLAUDE.md's "never log task line text" rule is about this crate's own
/// spans/events incidentally capturing IPC argument content, not about a purpose-built log bridge).
#[tracing::instrument(name = "ipc.ui_log", skip_all, fields(level = %level))]
#[tauri::command]
pub fn ui_log(
    level: String,
    message: String,
    fields: Option<serde_json::Value>,
) -> Result<(), String> {
    ui_log_inner(&level, &message, fields)
}

fn ui_log_inner(
    level: &str,
    message: &str,
    fields: Option<serde_json::Value>,
) -> Result<(), String> {
    // Serialized once, up front: `serde_json::Value::to_string()` is compact JSON (`Display`,
    // not `Debug`), so the field lands in the log as real JSON text instead of Rust debug syntax.
    let fields_json = fields
        .as_ref()
        .map_or_else(|| "null".to_owned(), ToString::to_string);
    match level {
        "error" => log_ui_error(message, &fields_json),
        "warn" => log_ui_warn(message, &fields_json),
        "debug" => log_ui_debug(message, &fields_json),
        "trace" => log_ui_trace(message, &fields_json),
        _ => log_ui_info(message, &fields_json),
    }
    Ok(())
}

/// One event-emitting function per level (never a bare `tracing::*!` call inside `ui_log_inner`'s
/// own `match` — that costs `clippy::cognitive_complexity` points, the same reason
/// `crates/txtodo-daemon/src/mutation.rs`'s `log_mutation_ops` is its own function).
fn log_ui_error(message: &str, fields_json: &str) {
    tracing::error!(fields = fields_json, "{message}");
}

fn log_ui_warn(message: &str, fields_json: &str) {
    tracing::warn!(fields = fields_json, "{message}");
}

fn log_ui_info(message: &str, fields_json: &str) {
    tracing::info!(fields = fields_json, "{message}");
}

fn log_ui_debug(message: &str, fields_json: &str) {
    tracing::debug!(fields = fields_json, "{message}");
}

fn log_ui_trace(message: &str, fields_json: &str) {
    tracing::trace!(fields = fields_json, "{message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;

    /// Proves `ui_log`'s `ipc.ui_log` span and its forwarded event actually reach a real
    /// subscriber — not just "it compiled" — the same `txtodo_telemetry::testing::LogSink` +
    /// `with_span_events(FmtSpan::CLOSE)` pattern `crates/txtodo-mcp/tests/smoke.rs`'s
    /// `mcp_call_span_names_tool_and_records_principal` test uses (a span with no event inside it
    /// never otherwise reaches the writer). Also doubles as the empirical proof that
    /// `#[tracing::instrument]` above `#[tauri::command]` works for a Tauri command: `ui_log` is
    /// called directly here exactly as `generate_handler!` would call it.
    #[test]
    fn ui_log_emits_a_named_span_and_the_forwarded_message() {
        let sink = txtodo_telemetry::testing::LogSink::new();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(sink.clone()),
        );
        let _guard = tracing::subscriber::set_default(subscriber);

        ui_log(
            "warn".to_owned(),
            "quick_add_hotkey_failed".to_owned(),
            Some(serde_json::json!({"attempt": 2})),
        )
        .expect("ui_log never fails");

        let text = sink.captured_text();
        let span_line = text
            .lines()
            .find(|l| l.contains("\"ipc.ui_log\""))
            .unwrap_or_else(|| panic!("no ipc.ui_log span line in captured output: {text}"));
        let span_value: serde_json::Value = serde_json::from_str(span_line).expect("valid JSON");
        assert_eq!(span_value["span"]["name"], "ipc.ui_log", "{span_line}");
        assert_eq!(span_value["span"]["level"], "warn", "{span_line}");
        let event_line = text
            .lines()
            .find(|l| l.contains("quick_add_hotkey_failed"))
            .unwrap_or_else(|| panic!("no forwarded message line in captured output: {text}"));
        let event_value: serde_json::Value = serde_json::from_str(event_line).expect("valid JSON");
        assert_eq!(
            event_value["fields"]["message"], "quick_add_hotkey_failed",
            "{event_line}"
        );
        // `fields` rides as a nested, already-serialized JSON string (see `ui_log_inner`), so it
        // is parsed a second time here rather than compared as a substring.
        let nested_fields: serde_json::Value = serde_json::from_str(
            event_value["fields"]["fields"]
                .as_str()
                .expect("string field"),
        )
        .expect("nested fields value is valid JSON");
        assert_eq!(nested_fields["attempt"], 2, "{event_line}");
    }
}
