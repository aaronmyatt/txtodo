//! Tauri-managed state: the daemon config, the current connectivity status, and the (possibly
//! absent) connected client. `tokio::sync::Mutex` (not `std::sync::Mutex`) because commands hold
//! the guard across `.await` points while making an RPC.

use crate::config::DesktopConfig;
use crate::daemon::DaemonClient;
use crate::status::DaemonStatus;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

/// State handed to every Tauri command via `tauri::State`.
pub struct AppState {
    /// Spawn-timeout/daemon-bin knobs; `config.workspace` is only the startup default now (ADR
    /// 0025, task `desktop-workspace-switcher`) — `current_workspace` below is the live value.
    pub config: DesktopConfig,
    /// Current connectivity state, mirrored to the frontend as a `daemon-status` event.
    pub status: Mutex<DaemonStatus>,
    /// The connected client once `Connected`; `None` while `Spawning`/`Connecting`/`Dead`. One
    /// client now serves every registered workspace (the global daemon) — `switch_workspace`
    /// changes which one its requests target via `DaemonClient::switch_workspace`, no reconnect.
    pub client: Mutex<Option<DaemonClient>>,
    /// The workspace every command currently targets — starts as `config.workspace`, changed by
    /// the `switch_workspace` command. Kept separate from `client` so `workspace_root` and the
    /// switcher UI can read it without locking the (possibly `None`, possibly mid-RPC) client.
    pub current_workspace: Mutex<PathBuf>,
    /// Whether the main window's edit popover currently has an unsaved edit
    /// (tasks/desktop-quick-add/notes.md: the global hotkey focuses the main window instead of
    /// opening quick-add while this is true). A plain `AtomicBool`, not a `tokio::sync::Mutex`:
    /// the global-shortcut handler that reads it is a synchronous, non-async callback
    /// (`tauri_plugin_global_shortcut`'s `on_shortcut`), so it needs a lock-free read.
    pub main_popover_dirty: AtomicBool,
}

impl AppState {
    /// Fresh state for `config`, starting `Connecting` with no client yet.
    pub fn new(config: DesktopConfig) -> AppState {
        let current_workspace = Mutex::new(config.workspace.clone());
        AppState {
            config,
            status: Mutex::new(DaemonStatus::Connecting),
            client: Mutex::new(None),
            current_workspace,
            main_popover_dirty: AtomicBool::new(false),
        }
    }
}
