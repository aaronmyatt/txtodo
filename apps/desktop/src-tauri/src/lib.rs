//! Tauri 2 desktop shell: a thin gRPC bridge to `txtodod` (design §5/§7, plan M7). The Rust
//! side owns the daemon connection; the Svelte frontend only calls the commands in
//! [`commands`] and listens for the `daemon-status`/`daemon-change` events — it never opens a
//! file or a socket itself.
#![allow(clippy::print_stderr)] // startup failure's only human-output path, like txtodod's main.rs

mod commands;
mod commands_activity;
mod commands_connect;
mod commands_notes;
mod commands_pairing;
mod commands_tokens;
mod commands_ui_log;
mod commands_universal;
mod commands_version;
mod commands_window;
mod commands_workspace;
// `pub` (not `mod`): the `e2e-bridge` feature's `src/bin/e2e_bridge.rs` binary is a separate crate
// target that only sees this library's public surface, and it reuses these DTOs and their
// `From<pb::...>` conversions directly rather than re-deriving them (tasks/desktop-playwright-
// tests/notes.md's harness). Every type here was already effectively public — it's exactly what
// crosses the Tauri IPC bridge to the frontend — so this doesn't newly expose anything.
pub mod dto;
mod dto_activity;
mod dto_notes;
mod dto_pairing;
mod dto_tokens;
mod dto_universal;
mod dto_workspace;
mod quick_add;
mod state;
mod status;
mod tray;

// Public so the integration test in `tests/` (and, later, other desktop-side crates) can drive
// `ensure_daemon`/`DaemonClient` directly without going through the Tauri command bridge.
pub mod config;
pub mod daemon;

use config::DesktopConfig;
use state::AppState;
use std::path::PathBuf;
use tauri::Manager;

/// Builds and runs the Tauri application: installs the tracing subscriber, manages [`AppState`],
/// kicks off the first connect/spawn in the background so startup never blocks on the daemon, and
/// registers every command in [`commands`].
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Held for the rest of `run`, the same "own the process's human-output window" pattern
    // `txtodo-cli`/`txtodo-mcp`'s own `main`s use for their own `_log_guard` — `tauri::Builder::
    // run` blocks until the app exits, so dropping this at the end of `run` (not `.setup()`, which
    // returns immediately) keeps the JSON writer's buffer alive for the app's whole lifetime.
    // Plain `init` (JSON file + pretty stderr), not `init_file_only`: unlike `txtodo-tui`, this
    // process never owns a raw-mode/alternate-screen terminal (it's a normal windowed GUI app,
    // launched either via `tauri dev`'s own terminal or as a packaged bundle where stderr is
    // simply discarded) — see `tasks/logging-desktop/notes.md` for the check against how this app
    // is actually launched. `.ok()`: a dead logger must never stop the app from starting, matching
    // `txtodo-tui`'s own `init_file_only().ok()` fallback-tolerant style.
    let _log_guard =
        txtodo_telemetry::init("desktop", &config::global_state_dir().join("logs")).ok();
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // https://v2.tauri.app/plugin/global-shortcut/ — backs the quick-add hotkey (`quick_add`).
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::daemon_status,
            commands::retry_connect,
            commands::workspace_root,
            commands::skill_hint,
            commands::set_main_popover_dirty,
            commands::list_files,
            commands::get_file,
            commands::watch,
            commands::apply,
            commands::history,
            commands::resolve,
            commands::list_conflicts,
            commands_ui_log::ui_log,
            commands_version::build_info,
            commands_window::set_pinned,
            commands_workspace::list_workspaces,
            commands_workspace::workspace_layout,
            commands_workspace::add_workspace,
            commands_workspace::remove_workspace,
            commands_workspace::switch_workspace,
            commands_universal::universal_tasks,
            commands_notes::get_notes,
            commands_notes::edit_notes,
            commands_pairing::pair_offer,
            commands_pairing::pair_accept,
            commands_pairing::pair_confirm_sas,
            commands_tokens::token_create,
            commands_tokens::token_list,
            commands_tokens::token_revoke,
            commands_activity::op_log,
            commands_activity::op_log_all,
        ])
        .build(tauri::generate_context!())
        .map(|app| {
            app.run(|handle, event| {
                #[cfg(target_os = "macos")]
                if let tauri::RunEvent::Reopen {
                    has_visible_windows: false,
                    ..
                } = event
                {
                    tray::show_main_window(handle);
                }
            });
        });
    if let Err(e) = result {
        eprintln!("desktop: {e}");
        std::process::exit(1);
    }
}

/// `.setup()` body, pulled out of [`run`] to keep it under clippy's `too_many_lines` budget.
/// Manages [`AppState`], creates the quick-add window/shortcut and tray, and starts the first
/// daemon connect in the background.
/// Ref: https://docs.rs/tauri/2/tauri/struct.Builder.html#method.setup
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // No explicit workspace: start on the default one (task default-workspace).
    let start = workspace_override().or_else(|| Some(config::default_workspace_dir()));
    app.manage(AppState::new(DesktopConfig::with_optional_workspace(start)));
    quick_add::create_window(app.handle())?;
    quick_add::register_shortcut(app.handle())?;
    tray::create_tray(app.handle())?;
    install_hide_not_quit(app.handle());
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        if let Err(e) = commands_connect::connect_and_store(&handle, &state).await {
            log_startup_connect_failed(&e);
            commands::set_status(&handle, &state, status::DaemonStatus::Dead).await;
        }
    });
    Ok(())
}

/// The startup daemon-connect attempt (`.setup()`, above) used to discard its `Result` entirely
/// (`let _ = commands_connect::connect_and_store(...).await`) — a failed boot connect left no
/// trace anywhere, not even a log line, since nothing else observes this background task. Logged
/// here, and (task `desktop-cold-boot-dead-status`) `.setup()`'s own error branch now also sets
/// `DaemonStatus::Dead` directly — `connect_and_store` itself only ever sets `Spawning`/
/// `Connecting`/`Connected` (see its own `set_status` calls), never `Dead` on its own error path,
/// so without this the UI was left on whatever status it last painted until a human noticed and
/// clicked the reconnect banner's Retry button (`commands::retry_connect`, whose own
/// `retry_connect_inner` already set `Dead` on *its* failure — this makes the cold-boot path
/// consistent with that, not a new mechanism). Isolated in its own function, not a bare
/// `tracing::warn!` inside the `if let` above, matching this pass's own `#[instrument]`-adjacent
/// style (no span here to protect a budget against, but consistent with
/// `crates/txtodo-daemon/src/mutation.rs`'s `log_mutation_ops` pattern regardless).
fn log_startup_connect_failed(e: &daemon::DaemonError) {
    tracing::warn!(error = %e, "startup_connect_failed");
}

/// Hide-not-quit (task `desktop-always-on`): closing the main window hides it instead of quitting
/// the app — the tray icon (`tray.rs`) is what keeps the process (and the daemon connection)
/// alive afterward, and its own "Quit" menu item is the only remaining path that actually calls
/// `app.exit(0)`. A no-op if the main window doesn't exist yet (it always does by the time
/// `.setup()` runs, since Tauri creates the windows declared in `tauri.conf.json` before calling
/// it — this `if let` is defensive, not expected to ever miss).
fn install_hide_not_quit(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let hidden = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = hidden.hide();
        }
    });
}

/// An explicit workspace to start on: `TXTODO_WORKSPACE` if set (dev/test override), else none —
/// never the launch directory, which is `/` for an app opened from Finder or the dock. The caller
/// then falls back to the default workspace.
fn workspace_override() -> Option<PathBuf> {
    std::env::var_os("TXTODO_WORKSPACE").map(PathBuf::from)
}
