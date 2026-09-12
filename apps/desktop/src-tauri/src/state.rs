//! Tauri-managed state: the daemon config, the current connectivity status, and the (possibly
//! absent) connected client. `tokio::sync::Mutex` (not `std::sync::Mutex`) because commands hold
//! the guard across `.await` points while making an RPC.

use crate::config::DesktopConfig;
use crate::daemon::DaemonClient;
use crate::status::DaemonStatus;
use tokio::sync::Mutex;

/// State handed to every Tauri command via `tauri::State`.
pub struct AppState {
    /// Workspace and spawn-timeout knobs.
    pub config: DesktopConfig,
    /// Current connectivity state, mirrored to the frontend as a `daemon-status` event.
    pub status: Mutex<DaemonStatus>,
    /// The connected client once `Connected`; `None` while `Spawning`/`Connecting`/`Dead`.
    pub client: Mutex<Option<DaemonClient>>,
}

impl AppState {
    /// Fresh state for `config`, starting `Connecting` with no client yet.
    pub fn new(config: DesktopConfig) -> AppState {
        AppState {
            config,
            status: Mutex::new(DaemonStatus::Connecting),
            client: Mutex::new(None),
        }
    }
}
