//! Desktop shell configuration: the workspace root and the daemon-spawn timing knob.
//! ADR 0010 fixes the socket layout; everything else here is just how long we wait.

use std::path::PathBuf;
use std::time::Duration;

/// The state directory name under the workspace root, matching
/// `crates/txtodo-daemon`'s `walker::STATE_DIR`.
const STATE_DIR: &str = ".txtodo";

/// The socket file name under the state directory (ADR 0010).
const SOCKET_FILE: &str = "txtodod.sock";

/// Runtime configuration for the daemon bridge: which workspace to talk to and how long
/// `ensure_daemon` waits for a freshly spawned `txtodod` to bind its socket.
#[derive(Debug, Clone)]
pub struct DesktopConfig {
    /// Workspace directory (contains `todo.txt` / `.txtodo/`, ADR 0010).
    pub workspace: PathBuf,
    /// Upper bound on waiting for the socket to appear after spawning `txtodod`.
    pub spawn_timeout: Duration,
    /// Overrides the `txtodod` program looked up when spawning; `None` means "resolve
    /// `txtodod` on `PATH`", like a normal install. Tests point this at the freshly built
    /// debug binary instead, without a Cargo dependency edge on `txtodo-daemon`.
    pub daemon_bin: Option<PathBuf>,
}

impl DesktopConfig {
    /// Default spawn timeout: generous enough for a debug build on a slow CI runner
    /// (`crates/txtodo-daemon/tests/support` uses 120 s for the same reason).
    pub const DEFAULT_SPAWN_TIMEOUT: Duration = Duration::from_secs(30);

    /// Configuration for `workspace` with the default spawn timeout and no `daemon_bin` override.
    pub fn new(workspace: impl Into<PathBuf>) -> DesktopConfig {
        DesktopConfig {
            workspace: workspace.into(),
            spawn_timeout: DesktopConfig::DEFAULT_SPAWN_TIMEOUT,
            daemon_bin: None,
        }
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
