//! Desktop shell configuration: the workspace root and the daemon-spawn timing knob.
//! ADR 0010 fixes the socket layout; everything else here is just how long we wait.
//!
//! `global_socket_path` (ADR 0025, task `desktop-workspace-switcher`, M11) is this crate's own
//! copy of `crates/txtodo-cli/src/config.rs::global_socket_path`'s logic — same env vars, same
//! XDG fallback chain — kept separate rather than a dependency on `txtodo-cli` (a binary crate,
//! not meant to be linked) or `txtodo-daemon` (its `workspace_registry_paths` module pulls in the
//! whole daemon dependency graph for one path function).

use std::path::PathBuf;
use std::time::Duration;

/// The state directory name under the workspace root, matching
/// `crates/txtodo-daemon`'s `walker::STATE_DIR`.
const STATE_DIR: &str = ".txtodo";

/// The socket file name under the state directory (ADR 0010).
const SOCKET_FILE: &str = "txtodod.sock";

/// The one device-global socket's path (ADR 0025): `$TXTODO_SOCKET` if set, else
/// `$XDG_DATA_HOME`/`%LOCALAPPDATA%`/`~/.local/share` + `txtodo/txtodod.sock`, falling back to
/// the cwd when none of those resolve — mirrors `txtodo-daemon`'s `workspace_registry_paths::
/// global_socket_path` (`legacy_dir: None` case) and `txtodo-cli`'s own copy of the same logic.
pub fn global_socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("TXTODO_SOCKET") {
        return PathBuf::from(p);
    }
    global_state_dir().join(SOCKET_FILE)
}

/// The device-global state directory (parent of [`global_socket_path`]'s default) — where
/// `ensure_daemon`'s client-side no-double-spawn lock lives now that it guards one global daemon
/// instead of one per workspace.
pub fn global_state_dir() -> PathBuf {
    data_dir().join("txtodo")
}

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .or_else(|| std::env::var_os("LOCALAPPDATA"))
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_e| PathBuf::from(".")))
}

/// Runtime configuration for the daemon bridge: which workspace to talk to and how long
/// `ensure_daemon` waits for a freshly spawned `txtodod` to bind its socket.
#[derive(Debug, Clone)]
pub struct DesktopConfig {
    /// Workspace directory (contains `todo.txt` / `.txtodo/`, ADR 0010) — the startup default for
    /// `AppState::current_workspace`, not which daemon to dial (that's always the global one now).
    pub workspace: PathBuf,
    /// Upper bound on waiting for the socket to appear after spawning `txtodod`.
    pub spawn_timeout: Duration,
    /// Overrides the `txtodod` program looked up when spawning; `None` means "resolve
    /// `txtodod` on `PATH`", like a normal install. Tests point this at the freshly built
    /// debug binary instead, without a Cargo dependency edge on `txtodo-daemon`.
    pub daemon_bin: Option<PathBuf>,
    /// Overrides [`global_socket_path`]'s result; `None` means "resolve it normally". A field
    /// here (not a process env var read at call time) so parallel tests each get their own
    /// hermetic global socket without racing to mutate this process' shared environment — the
    /// same reason `daemon_bin` above is a field, not a `$PATH` lookup with an env override.
    pub global_socket_override: Option<PathBuf>,
    /// As `global_socket_override`, for the daemon's `$TXTODO_REGISTRY_DB` — passed to the spawned
    /// child's own environment (safe: a `Command`'s env is never shared with this process'), not
    /// read from this process' environment.
    pub global_registry_override: Option<PathBuf>,
}

impl DesktopConfig {
    /// Default spawn timeout: generous enough for a debug build on a slow CI runner
    /// (`crates/txtodo-daemon/tests/support` uses 120 s for the same reason).
    pub const DEFAULT_SPAWN_TIMEOUT: Duration = Duration::from_secs(30);

    /// Configuration for `workspace` with the default spawn timeout and no overrides.
    pub fn new(workspace: impl Into<PathBuf>) -> DesktopConfig {
        DesktopConfig {
            workspace: workspace.into(),
            spawn_timeout: DesktopConfig::DEFAULT_SPAWN_TIMEOUT,
            daemon_bin: None,
            global_socket_override: None,
            global_registry_override: None,
        }
    }

    /// The global socket this config's daemon connection targets: `global_socket_override` if
    /// set, else [`global_socket_path`]'s real, env-resolved default.
    pub fn resolved_global_socket(&self) -> PathBuf {
        self.global_socket_override
            .clone()
            .unwrap_or_else(global_socket_path)
    }

    /// ADR 0010: `<workspace>/.txtodo/txtodod.sock`.
    pub fn socket_path(&self) -> PathBuf {
        self.state_dir().join(SOCKET_FILE)
    }

    /// `<workspace>/.txtodo`, created by `ensure_daemon` if missing (the daemon also owns it,
    /// so this only needs to exist far enough to hold the client-side spawn lock file).
    pub fn state_dir(&self) -> PathBuf {
        self.workspace.join(STATE_DIR)
    }
}
