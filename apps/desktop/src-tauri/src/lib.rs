//! Tauri 2 desktop shell: a thin gRPC bridge to `txtodod` (design §5/§7, plan M7). The Rust
//! side owns the daemon connection; the Svelte frontend only calls the commands in
//! [`commands`] and listens for the `daemon-status`/`daemon-change` events — it never opens a
//! file or a socket itself.
#![allow(clippy::print_stderr)] // startup failure's only human-output path, like txtodod's main.rs

mod commands;
mod dto;
mod state;
mod status;

// Public so the integration test in `tests/` (and, later, other desktop-side crates) can drive
// `ensure_daemon`/`DaemonClient` directly without going through the Tauri command bridge.
pub mod config;
pub mod daemon;

use config::DesktopConfig;
use state::AppState;
use std::path::PathBuf;
use tauri::Manager;

/// Builds and runs the Tauri application: manages [`AppState`], kicks off the first
/// connect/spawn in the background so startup never blocks on the daemon, and registers every
/// command in [`commands`].
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(AppState::new(DesktopConfig::new(workspace_dir())));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                let _ = commands::connect_and_store(&handle, &state).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::daemon_status,
            commands::retry_connect,
            commands::list_files,
            commands::get_file,
            commands::watch,
            commands::apply,
            commands::history,
            commands::resolve,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("desktop: {e}");
        std::process::exit(1);
    }
}

/// The workspace to talk to: `TXTODO_WORKSPACE` if set (dev/test override), else the current
/// directory at launch.
fn workspace_dir() -> PathBuf {
    std::env::var_os("TXTODO_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_e| PathBuf::from(".")))
}
