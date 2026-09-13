//! Tauri-managed state: the daemon config, the current connectivity status, and the (possibly
//! absent) connected client. `tokio::sync::Mutex` (not `std::sync::Mutex`) because commands hold
//! the guard across `.await` points while making an RPC.

use crate::config::DesktopConfig;
use crate::daemon::DaemonClient;
use crate::status::DaemonStatus;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

/// State handed to every Tauri command via `tauri::State`.
pub struct AppState {
    /// Workspace and spawn-timeout knobs.
    pub config: DesktopConfig,
    /// Current connectivity state, mirrored to the frontend as a `daemon-status` event.
    pub status: Mutex<DaemonStatus>,
    /// The connected client once `Connected`; `None` while `Spawning`/`Connecting`/`Dead`.
    pub client: Mutex<Option<DaemonClient>>,
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
        AppState {
            config,
            status: Mutex::new(DaemonStatus::Connecting),
            client: Mutex::new(None),
            main_popover_dirty: AtomicBool::new(false),
        }
    }
}
