//! Test-only HTTP bridge standing in for the Tauri IPC layer, so Playwright (a plain browser, no
//! Tauri runtime) can drive the real frontend against a real `txtodod`
//! (tasks/desktop-playwright-tests/notes.md: "the harness runs the real daemon on a tempdir
//! workspace ... the code under test is the real reconciler, real file bytes, real op log").
//!
//! This reuses `desktop_lib`'s own `DaemonClient` and `dto` conversions directly — it is
//! deliberately NOT a second gRPC client or a re-implementation of `commands.rs`'s logic, just
//! that same logic restated over plain JSON/HTTP instead of Tauri's IPC, for the handful of
//! commands the e2e suite's scenarios need (`apps/desktop/e2e/shim/core.ts` is the frontend half).
//!
//! Guarded so it can never ship enabled to production: this binary only exists behind the
//! `e2e-bridge` Cargo feature (`Cargo.toml`'s `required-features` on this `[[bin]]`), which a
//! plain `cargo build -p desktop` never enables.
//!
//! Run: `TXTODO_WORKSPACE=<dir> E2E_BRIDGE_PORT=<port> TXTODO_E2E_GLOBAL_DIR=<dir2>
//! cargo run -p desktop --features e2e-bridge --bin e2e_bridge` — `apps/desktop/e2e/fixtures.ts`
//! does exactly this.
//! Ref: <https://docs.rs/axum>

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use desktop_lib::config::DesktopConfig;
use desktop_lib::daemon::{self, DaemonClient, DaemonError};
use desktop_lib::dto::{
    ApplyResultDto, FileContentsDto, FileInfoDto, HistoryDto, MutationDto, NotesDocDto,
    ResolutionDto, ReviewFlagDto, TaskRefDto,
};
use serde::Deserialize;
use serde_json::Value;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use txtodo_proto::v1 as pb;

#[path = "e2e_bridge/workspace.rs"]
mod workspace;
use workspace::dispatch_workspace_cmd;

#[path = "e2e_bridge/activity.rs"]
mod activity;
use activity::cmd_op_log_all;

#[path = "e2e_bridge/conflict.rs"]
mod conflict;
use conflict::cmd_debug_raise_conflict;

#[path = "e2e_bridge/refdir.rs"]
mod refdir;
use refdir::dispatch_notes_cmd;

/// The connected client plus the workspace root, so `debug_raise_conflict` can open its own
/// connection to `.txtodo/oplog.db` alongside the daemon's (same pattern as
/// `crates/txtodo-daemon/tests/grpc.rs::raise_flag`).
struct BridgeState {
    client: Mutex<DaemonClient>,
    workspace: PathBuf,
}

type Shared = Arc<BridgeState>;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("E2E_BRIDGE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4567);

    // Isolate this bridge's daemon from the REAL, machine-global socket/registry.db
    // (`$XDG_DATA_HOME/txtodo/...`): without an override, `ensure_daemon` below binds true global
    // mode onto that shared state, so every Playwright fixture would leak a workspace-registry row
    // into a human's actual desktop app and share one `txtodod` across the whole test run (see
    // `fixtures.ts`'s module doc). `TXTODO_E2E_GLOBAL_DIR` is a tempdir `fixtures.ts` creates and
    // tears down alongside the workspace — a SIBLING of it, not nested inside: `notes-create.spec.
    // ts`'s negative assertion checks the workspace root's own top-level listing is exactly
    // `{todo.txt, .txtodo}` before any user action, so this daemon's global state (socket,
    // registry.db, identity.db, pidfile, logs) must never appear inside the workspace tree at all.
    let global_state_dir = std::env::var("TXTODO_E2E_GLOBAL_DIR")
        .unwrap_or_else(|_| panic!("e2e_bridge: TXTODO_E2E_GLOBAL_DIR must be set"));
    // `TXTODO_WORKSPACE` picks the workspace; without it the bridge stands in for a fresh profile
    // and uses the default workspace, which the daemon creates beside its socket (task
    // default-workspace: `txtodo_workspace_paths::default_workspace_dir_for`).
    let workspace =
        std::env::var("TXTODO_WORKSPACE").unwrap_or_else(|_| format!("{global_state_dir}/default"));
    let mut cfg = DesktopConfig::new(workspace.clone());
    cfg.global_socket_override = Some(PathBuf::from(&global_state_dir).join("txtodod.sock"));
    cfg.global_registry_override = Some(PathBuf::from(&global_state_dir).join("registry.db"));
    let sock = daemon::ensure_daemon(&cfg)
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: ensure_daemon: {e}"));
    // `ensure_daemon` spawns `txtodod` in true global mode (no `--dir`) — no workspace is open at
    // all yet, so `None` ("the sole open workspace") has nothing to resolve to. A `Path` selector
    // is what actually gets it opened (`WorkspaceCatalog::resolve` auto-registers/opens an unknown
    // directory), the same selector `commands.rs::connect_and_store` builds for the real app.
    let selector = Some(pb::WorkspaceSelector {
        selector: Some(pb::workspace_selector::Selector::Path(workspace.clone())),
    });
    let mut client = DaemonClient::connect(&sock, selector)
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: connect: {e}"));
    client
        .wait_until_ready()
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: wait_until_ready: {e}"));
    // `wait_until_ready` only lists the registry, so nothing has opened this workspace yet. A call
    // that carries the selector does (it promotes the workspace and waits for its open); without it
    // `.txtodo/` may not exist when a fixture reaches for the store straight away
    // (`debug_raise_conflict`). Task default-workspace made this visible: an unselected call now
    // goes to the default workspace instead of opening the sole registered one.
    client
        .list_files()
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: opening the workspace: {e}"));
    let shared: Shared = Arc::new(BridgeState {
        client: Mutex::new(client),
        workspace: PathBuf::from(workspace),
    });

    // No auth, no TLS: binds to loopback only and only ever runs for the lifetime of one
    // Playwright test process on a throwaway tempdir workspace. CORS is wide open (`*`) for the
    // same reason: the page under test is served by Vite on a *different* port
    // (playwright.config.ts), so every `/invoke` call is cross-origin from the browser's
    // perspective, and a JSON POST body triggers a preflight `OPTIONS` — both need an explicit
    // answer here since this binary talks to nothing but a loopback-only Playwright browser.
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/invoke", post(invoke).options(preflight))
        .layer(middleware::from_fn(add_cors_headers))
        .with_state(shared);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: bind {addr}: {e}"));
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("e2e_bridge: serve: {e}"));
}

/// Answers a CORS preflight `OPTIONS /invoke`; `add_cors_headers` below fills in the actual
/// headers a browser checks (this handler only needs to return a success status).
async fn preflight() -> StatusCode {
    StatusCode::NO_CONTENT
}

/// Adds permissive CORS headers to every response — see the `Router` construction in `main` for
/// why this loopback-only, test-only binary needs them at all.
async fn add_cors_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static("content-type"),
    );
    response
}

/// Mirrors `@tauri-apps/api/core`'s `invoke(cmd, args)` shape exactly, so
/// `apps/desktop/e2e/shim/core.ts` can be a nearly-transparent stand-in for the real thing.
#[derive(Deserialize)]
struct InvokeReq {
    cmd: String,
    #[serde(default)]
    args: Value,
}

/// Every failure here becomes a `400` whose body is the error's `Display` text — matching how a
/// rejected Tauri `invoke()` surfaces a plain string to the frontend's `catch (e) { String(e) }`.
struct ApiError(String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, self.0).into_response()
    }
}

impl From<DaemonError> for ApiError {
    fn from(e: DaemonError) -> ApiError {
        ApiError(e.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> ApiError {
        ApiError(format!("e2e_bridge: bad args/response: {e}"))
    }
}

fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, ApiError> {
    serde_json::from_value(args).map_err(ApiError::from)
}

/// The handful of commands the six Playwright scenarios need (tasks/desktop-playwright-tests):
/// `list_files`, `get_file`, `apply`, `history`, `list_conflicts`, `resolve`, `get_notes`,
/// `edit_notes`, plus the test-only `debug_raise_conflict` (conflict.spec.ts — see that
/// function's doc comment). `watch`/`daemon_status`/pairing/tokens/activity are unused by those
/// scenarios and intentionally not wired here — `e2e/shim/event.ts` polls the commands above
/// instead of using a real `Watch` stream (see that file's module doc for why that's still "real
/// reconciliation"). Dispatches to one `cmd_*` function per command (kept separate, not inlined,
/// so no single match arm grows past this crate's line-count lint).
///
/// `workspace_root` was added by tasks/desktop-visual-regression: `e2e/shim/core.ts` used to
/// hardcode `""` for it ("not exercised by the six e2e scenarios"), which is exactly what left
/// `DetailView.svelte`'s footer (`{absoluteRefDir}`) empty for that task's detail-view golden —
/// the one thing the real Tauri command surface has that those six scenarios happened never to
/// need. No daemon RPC involved: the bridge already knows its own workspace root.
///
/// `list_workspaces`/`add_workspace`/`remove_workspace`/`switch_workspace` were added by task
/// `desktop-workspace-nav-sidebar`: the first Playwright coverage of `WorkspaceSwitcher.svelte`
/// needed them, restating `commands_workspace.rs`'s own bodies over this bridge's `DaemonClient`
/// exactly like every other `cmd_*` here restates `commands.rs`'s.
async fn invoke(
    State(state): State<Shared>,
    Json(req): Json<InvokeReq>,
) -> Result<Response, ApiError> {
    if req.cmd == "debug_raise_conflict" {
        let value = cmd_debug_raise_conflict(&state.workspace, req.args)?;
        return Ok(Json(value).into_response());
    }
    if req.cmd == "workspace_root" {
        let value = Value::String(state.workspace.display().to_string());
        return Ok(Json(value).into_response());
    }
    // Kept as a separate function, not a 5th arm/branch here: clippy's cognitive-complexity lint
    // was already at this crate's budget with just the two `if`s above plus the match `invoke_core`
    // now owns unchanged, so the new workspace-command path gets its own branch in its own
    // function rather than adding a branch to this one.
    if WORKSPACE_CMDS.contains(&req.cmd.as_str()) {
        let mut client = state.client.lock().await;
        let value = dispatch_workspace_cmd(&mut client, &req.cmd, req.args).await?;
        return Ok(Json(value).into_response());
    }
    // Same reasoning as the workspace-command branch above: its own branch, not a 9th arm in
    // `invoke_core`'s match, to stay under this crate's cognitive-complexity budget.
    if req.cmd == "op_log_all" {
        let mut client = state.client.lock().await;
        let value = cmd_op_log_all(&mut client).await?;
        return Ok(Json(value).into_response());
    }
    invoke_core(state, req).await
}

const WORKSPACE_CMDS: [&str; 5] = [
    "list_workspaces",
    "workspace_layout",
    "add_workspace",
    "remove_workspace",
    "switch_workspace",
];

async fn invoke_core(state: Shared, req: InvokeReq) -> Result<Response, ApiError> {
    let mut client = state.client.lock().await;
    let value: Value = match req.cmd.as_str() {
        "list_files" => cmd_list_files(&mut client).await?,
        "get_file" => cmd_get_file(&mut client, req.args).await?,
        "apply" => cmd_apply(&mut client, req.args).await?,
        "history" => cmd_history(&mut client, req.args).await?,
        "list_conflicts" => cmd_list_conflicts(&mut client, req.args).await?,
        "resolve" => cmd_resolve(&mut client, req.args).await?,
        "get_notes" | "edit_notes" | "ref_dir" => {
            dispatch_notes_cmd(&mut client, &req.cmd, req.args).await?
        }
        other => {
            return Err(ApiError(format!(
                "e2e_bridge: unsupported command {other:?}"
            )));
        }
    };
    Ok(Json(value).into_response())
}

async fn cmd_list_files(client: &mut DaemonClient) -> Result<Value, ApiError> {
    let resp = client.list_files().await?;
    let dtos: Vec<FileInfoDto> = resp.files.into_iter().map(FileInfoDto::from).collect();
    Ok(serde_json::to_value(dtos)?)
}

async fn cmd_get_file(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    struct Req {
        path: String,
    }
    let r: Req = parse(args)?;
    let resp = client.get_file(&r.path).await?;
    Ok(serde_json::to_value(FileContentsDto::from(resp))?)
}

async fn cmd_apply(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    struct Req {
        path: String,
        mutations: Vec<MutationDto>,
    }
    let r: Req = parse(args)?;
    let pb_req = pb::ApplyRequest {
        path: r.path,
        mutations: r.mutations.into_iter().map(pb::Mutation::from).collect(),
        agent: None,
        workspace: None,
        source: "desktop".to_owned(),
        dry_run: false,
    };
    let resp = client
        .apply(pb_req)
        .await
        .map_err(|e| ApiError(e.apply_text()))?;
    Ok(serde_json::to_value(ApplyResultDto::from(resp))?)
}

async fn cmd_history(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Req {
        path: String,
        task_id: String,
        limit: u32,
    }
    let r: Req = parse(args)?;
    let resp = client
        .history(pb::HistoryRequest {
            path: r.path,
            task_id: r.task_id,
            limit: r.limit,
            before_seq: 0,
            workspace: None,
        })
        .await?;
    Ok(serde_json::to_value(HistoryDto::from(resp))?)
}

async fn cmd_list_conflicts(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    struct Req {
        path: String,
    }
    let r: Req = parse(args)?;
    let resp = client.list_conflicts(&r.path).await?;
    let dtos: Vec<ReviewFlagDto> = resp.flags.into_iter().map(ReviewFlagDto::from).collect();
    Ok(serde_json::to_value(dtos)?)
}

async fn cmd_resolve(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    struct Req {
        path: String,
        task: TaskRefDto,
        resolution: ResolutionDto,
    }
    let r: Req = parse(args)?;
    let resp = client
        .resolve(pb::ResolveRequest {
            path: r.path,
            task: Some(r.task.into()),
            resolution: pb::Resolution::from(r.resolution) as i32,
            workspace: None,
            agent: None,
        })
        .await?;
    Ok(serde_json::to_value(ApplyResultDto::from(resp))?)
}

async fn cmd_get_notes(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    struct Req {
        task: TaskRefDto,
    }
    let r: Req = parse(args)?;
    let resp = client.get_notes(r.task.into()).await?;
    Ok(serde_json::to_value(NotesDocDto::from(resp))?)
}

async fn cmd_edit_notes(client: &mut DaemonClient, args: Value) -> Result<Value, ApiError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Req {
        task: TaskRefDto,
        new_text: String,
    }
    let r: Req = parse(args)?;
    let resp = client
        .edit_notes(pb::NotesEditRequest {
            task: Some(r.task.into()),
            new_text: r.new_text,
            workspace: None,
        })
        .await?;
    Ok(serde_json::to_value(ApplyResultDto::from(resp))?)
}
