//! Desktop shell configuration: the workspace root and the daemon-spawn timing knob.
//! ADR 0010 fixes the socket layout; everything else here is just how long we wait.
//!
//! `global_socket_path`/`global_state_dir` (ADR 0025, task `desktop-workspace-switcher`, M11)
//! delegate to `txtodo-workspace-paths` (task `daemon-paths-shared-crate`) — this crate
//! previously reimplemented the same fallback chain by hand, kept separate rather than a
//! dependency on `txtodo-cli` (a binary crate, not meant to be linked) or `txtodo-daemon` (its
//! `workspace_registry_paths` module pulls in the whole daemon dependency graph for one path
//! function). The shared crate has neither constraint, being a dependency-free leaf.

use std::path::{Path, PathBuf};
use std::time::Duration;
use txtodo_workspace_paths::RegistryEnv;

/// The one device-global socket's path (ADR 0025): `$TXTODO_SOCKET` if set, else
/// `$XDG_DATA_HOME`/`%LOCALAPPDATA%`/`~/.local/share` + `txtodo/txtodod.sock`, falling back to
/// the cwd when none of those resolve.
pub fn global_socket_path() -> PathBuf {
    let env = RegistryEnv::from_process().unwrap_or_default();
    txtodo_workspace_paths::global_socket_path(&env, None)
}

/// The device-global state directory (parent of [`global_socket_path`]'s default) — where
/// `ensure_daemon`'s client-side no-double-spawn lock lives now that it guards one global daemon
/// instead of one per workspace.
pub fn global_state_dir() -> PathBuf {
    let env = RegistryEnv::from_process().unwrap_or_default();
    txtodo_workspace_paths::global_state_dir(&env)
}

/// The bundled sidecar's expected path, if one exists (task `desktop-daemon-sidecar-bundle`):
/// beside this process' own executable, named `txtodod` (`.exe` on Windows). Tauri's
/// `externalBin` bundling (`tauri.conf.json`'s `bundle.externalBin: ["binaries/txtodod"]`) copies
/// the target-triple-suffixed binary staged at build time (`binaries/txtodod-<target-triple>`)
/// into the packaged app next to its own main executable, stripping the suffix for the platform
/// actually being built — the same "beside the binary, else PATH" resolution
/// `crates/txtodo-cli/src/commands/service.rs::txtodod_path` already uses for the CLI's own
/// sibling `txtodod`, extended here to the sidecar case. This also covers a plain dev checkout
/// (`cargo build --workspace` puts both `desktop` and `txtodod` in the same `target/debug/`
/// directory), so it subsumes the README's older "`txtodod` on `$PATH`" instruction rather than
/// only helping packaged builds. **Not verified against a real `tauri build` bundle** — this
/// sandbox has no GUI/bundler to produce and open one; see
/// `tasks/desktop-daemon-sidecar-bundle/notes.md` for what a human still needs to confirm.
fn sidecar_daemon_bin() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let sibling = sidecar_candidate(&exe);
    sibling.is_file().then_some(sibling)
}

/// The pure, filesystem-free half of [`sidecar_daemon_bin`]'s lookup: where the sidecar *would*
/// be for a given executable path, regardless of whether it actually exists there. Split out so
/// this naming logic is unit-testable without needing a real file on disk beside the test
/// binary's own (non-deterministic, per-run) path.
fn sidecar_candidate(exe: &Path) -> PathBuf {
    exe.with_file_name(format!("txtodod{}", std::env::consts::EXE_SUFFIX))
}

/// Runtime configuration for the daemon bridge: which workspace to talk to and how long
/// `ensure_daemon` waits for a freshly spawned `txtodod` to bind its socket.
#[derive(Debug, Clone)]
pub struct DesktopConfig {
    /// Workspace directory (contains `todo.txt` / `.txtodo/`, ADR 0010) — the startup default for
    /// `AppState::current_workspace`, not which daemon to dial (that's always the global one now).
    pub workspace: Option<PathBuf>,
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
    /// Delegates to the shared crate's constant (`txtodo_daemon_launch::LaunchConfig`'s own doc
    /// has the real reasoning: a full cold start replays every registered workspace's Loro mirror
    /// sequentially, a known, data-size-dependent cost, not a fixed one). This used to be its own
    /// separately-hardcoded 30s here — found stale and out of sync with the shared crate's value
    /// after `daemon/spawn.rs` migrated onto it (item 6), which meant this config's own override
    /// silently kept desktop on the old, too-short timeout regardless of the shared default.
    pub const DEFAULT_SPAWN_TIMEOUT: Duration =
        txtodo_daemon_launch::LaunchConfig::DEFAULT_SPAWN_TIMEOUT;

    /// Configuration for `workspace` with the default spawn timeout, no socket/registry
    /// overrides, and `daemon_bin` set to the bundled sidecar ([`sidecar_daemon_bin`]) if one is
    /// found beside this process' own executable, else `None` (bare `$PATH` lookup — a dev
    /// checkout with no sidecar). A test that wants a specific binary overrides `daemon_bin`
    /// directly on the returned value, same as before this existed.
    pub fn new(workspace: impl Into<PathBuf>) -> DesktopConfig {
        DesktopConfig::with_workspace(Some(workspace.into()))
    }

    /// As [`DesktopConfig::new`], but with no workspace selected: the app only reflects
    /// workspaces a human linked or the cli/tui touched, never one guessed from its launch
    /// directory (a Finder-launched app's cwd is `/`).
    pub fn unbound() -> DesktopConfig {
        DesktopConfig::with_workspace(None)
    }

    /// [`DesktopConfig::new`] for `Some`, [`DesktopConfig::unbound`] for `None`.
    pub fn with_optional_workspace(workspace: Option<PathBuf>) -> DesktopConfig {
        DesktopConfig::with_workspace(workspace)
    }

    fn with_workspace(workspace: Option<PathBuf>) -> DesktopConfig {
        DesktopConfig {
            workspace,
            spawn_timeout: DesktopConfig::DEFAULT_SPAWN_TIMEOUT,
            daemon_bin: sidecar_daemon_bin(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_candidate_is_named_txtodod_beside_the_given_executable() {
        let candidate = sidecar_candidate(Path::new(
            "/Applications/desktop.app/Contents/MacOS/desktop",
        ));
        let expected = PathBuf::from(format!(
            "/Applications/desktop.app/Contents/MacOS/txtodod{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert_eq!(candidate, expected);
    }

    #[test]
    fn sidecar_daemon_bin_is_none_when_no_sibling_binary_exists() {
        // The real cargo test binary's own directory (target/debug/deps/) never has a
        // "txtodod" file beside it (txtodod is built into target/debug/ directly, a sibling
        // directory, not deps/) — a real, always-true negative case, not a mock.
        assert_eq!(sidecar_daemon_bin(), None);
    }
}
