//! Tauri-managed state: the daemon config, the current connectivity status, and the (possibly
//! absent) connected client. `tokio::sync::Mutex` (not `std::sync::Mutex`) because the guard is
//! taken in async code. No command holds the `client` guard across its RPC: see
//! [`AppState::client_snapshot`].

use crate::config::DesktopConfig;
use crate::daemon::DaemonClient;
use crate::status::DaemonStatus;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use tauri::async_runtime::JoinHandle;
use tokio::sync::Mutex;

/// The one shared `Change` stream's forwarder task, if a live one exists. `commands.rs::watch_inner`
/// installs it; a reconnect (`commands_connect`) or a workspace switch (`commands_workspace`) stops
/// it, since its stream belongs to the old connection or the old workspace; and the forwarder
/// itself clears it when its stream ends (daemon restart), so the next `watch()` opens a fresh one
/// instead of trusting a flag nobody reset (root todo ids 01M2WK5DQQFAD15221N11V026W and
/// 01M2WK5DQQQRNTPEAAQW9SV7HG).
#[derive(Default)]
pub struct WatchSlot {
    /// Bumped on every install, so a forwarder that outlives its slot (aborted, or ended just as
    /// a newer one was installed) cannot clear its successor.
    generation: u64,
    task: Option<JoinHandle<()>>,
}

impl WatchSlot {
    /// True while a forwarder is installed and its stream has not ended.
    pub fn is_running(&self) -> bool {
        self.task.is_some()
    }

    /// The generation the next installed forwarder must carry into `install`/`ended`. Claimed
    /// before the task is spawned and held until `install`, under one lock, so a stream that ends
    /// at once waits for `install` rather than racing it.
    pub fn claim_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Makes `task` the running forwarder for `generation` (from `claim_generation`).
    pub fn install(&mut self, generation: u64, task: JoinHandle<()>) {
        debug_assert_eq!(generation, self.generation, "install without a fresh claim");
        self.task = Some(task);
    }

    /// Aborts the running forwarder, if any: its stream targets a connection or workspace that is
    /// no longer the current one.
    pub fn stop(&mut self) {
        // A bumped generation also disarms a forwarder that is between `message()` and `ended`.
        self.generation += 1;
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    /// The forwarder of `generation` saw its stream end. Clears the slot only when nothing newer
    /// was installed meanwhile.
    pub fn ended(&mut self, generation: u64) {
        if generation == self.generation {
            self.task = None;
        }
    }
}

/// State handed to every Tauri command via `tauri::State`.
pub struct AppState {
    /// Spawn-timeout/daemon-bin knobs; `config.workspace` is only an explicit override
    /// (`TXTODO_WORKSPACE`, tests) — `current_workspace` below is the live value.
    pub config: DesktopConfig,
    /// Current connectivity state, mirrored to the frontend as a `daemon-status` event.
    pub status: Mutex<DaemonStatus>,
    /// The connected client once `Connected`; `None` while `Spawning`/`Connecting`/`Dead`. One
    /// client now serves every registered workspace (the global daemon) — `switch_workspace`
    /// changes which one its requests target via `DaemonClient::switch_workspace`, no reconnect.
    pub client: Mutex<Option<DaemonClient>>,
    /// The workspace every command currently targets: `None` until a human links or picks one
    /// (`switch_workspace`) — the app never assumes one from its launch directory. Kept separate
    /// from `client` so `workspace_root` and the switcher UI can read it without locking the
    /// (possibly `None`, possibly mid-RPC) client.
    pub current_workspace: Mutex<Option<PathBuf>>,
    /// Whether the main window's edit popover currently has an unsaved edit
    /// (tasks/desktop-quick-add/notes.md: the global hotkey focuses the main window instead of
    /// opening quick-add while this is true). A plain `AtomicBool`, not a `tokio::sync::Mutex`:
    /// the global-shortcut handler that reads it is a synchronous, non-async callback
    /// (`tauri_plugin_global_shortcut`'s `on_shortcut`), so it needs a lock-free read.
    pub main_popover_dirty: AtomicBool,
    /// `watch`'s one shared `Change` stream + forwarder task for `client`'s current connection and
    /// current workspace (tasks/desktop-concurrent-edit-loss root cause 3: every `FileView`/
    /// `DetailView` mount or path-switch used to call `watch()` again, each opening its own
    /// never-cancelled forwarder, so one daemon change fired several `refreshDoc`s). A reconnect
    /// (cold boot or manual Retry) and `switch_workspace` both stop it, so the next `watch()`
    /// opens a fresh stream against the new client/workspace instead of a stale one; the
    /// forwarder clears it itself when its stream ends.
    pub watch: Mutex<WatchSlot>,
}

impl AppState {
    /// A clone of the connected client; the lock is released before this returns. Every command
    /// makes its RPC on the clone (code review 2026-09-20, finding 5): a call on a workspace that
    /// is still loading can sit in the daemon for up to two minutes, and while a command held this
    /// lock across it, every other command, the switcher's 2 s `list_workspaces` poll included,
    /// queued behind it and the window froze. The clone carries the selector of the moment, so a
    /// call in flight keeps its workspace when the human switches.
    pub async fn client_snapshot(&self) -> Result<DaemonClient, String> {
        self.client
            .lock()
            .await
            .clone()
            .ok_or_else(|| "daemon not connected".to_owned())
    }

    /// Fresh state for `config`, starting `Connecting` with no client yet.
    pub fn new(config: DesktopConfig) -> AppState {
        let current_workspace = Mutex::new(config.workspace.clone());
        AppState {
            config,
            status: Mutex::new(DaemonStatus::Connecting),
            client: Mutex::new(None),
            current_workspace,
            main_popover_dirty: AtomicBool::new(false),
            watch: Mutex::new(WatchSlot::default()),
        }
    }
}
