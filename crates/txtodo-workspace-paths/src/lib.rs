//! Where this device's global, per-user daemon state lives: the registry database, socket, pid
//! lock and log directory the single `txtodod` for this device binds — deliberately outside any
//! single workspace's `<workspace>/.txtodo/` (ADR 0010 fixes that path per workspace; device-
//! global state has no single workspace to live under). `$XDG_DATA_HOME` (or the platform
//! equivalent), not `$XDG_CONFIG_HOME`: this is generated, mutable state the daemon owns, not
//! something a human hand-edits like `config.toml`.
//!
//! Extracted into its own leaf crate (task `daemon-paths-shared-crate`, following the daemon-
//! consistency audit that also produced `ref:daemon-ready-log-ordering`) after this exact
//! fallback chain had been independently reimplemented three more times — `txtodo-cli`'s
//! `config.rs`, `txtodo-mcp`'s `global_socket.rs`, and `apps/desktop/src-tauri`'s `config.rs` —
//! each unable to depend on `txtodo-daemon` directly (`.claude/budgets.json`'s `allowedDeps` runs
//! the dependency the other way: the daemon depends on `txtodo-mcp`, not vice versa, and pulling
//! in the whole daemon crate for one path function was never the point anyway). This crate has
//! zero dependencies, so every one of `txtodo-daemon`, `txtodo-cli`, `txtodo-mcp` and
//! `apps/desktop` can depend on it directly instead of drifting independently.
//!
//! `--dir <workspace>`-started daemons (the legacy single-workspace bridge, kept for the huge
//! pre-existing single-workspace test suite and today's CLI) do **not** use the device-global
//! defaults below: `global_socket_path`/`registry_db_path_for` both take an optional `legacy_dir`
//! and fall back to the pre-existing `<dir>/.txtodo/{txtodod.sock, registry.db}` locations when
//! it is given, so every ephemeral-tmpdir-per-test daemon stays exactly as hermetic as before —
//! none of them touch this machine's real `$XDG_DATA_HOME/txtodo/` at all. Only a daemon started
//! with `--dir` omitted (true one-per-device mode) resolves the device-global defaults;
//! `$TXTODO_SOCKET`/`$TXTODO_REGISTRY_DB` remain escape hatches either way, checked before
//! `legacy_dir`.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The slice of the process environment this module reads. Built once by a future `main.rs`
/// wiring pass; tests build their own so no test depends on the real machine's environment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistryEnv {
    vars: BTreeMap<String, String>,
    cwd: PathBuf,
}

impl RegistryEnv {
    /// From explicit variables and a working directory (tests, and a future `main.rs`).
    pub fn new(vars: BTreeMap<String, String>, cwd: PathBuf) -> RegistryEnv {
        RegistryEnv { vars, cwd }
    }
    /// Snapshot of the real process. Non-UTF-8 values are dropped, same as `txtodo-cli`'s `Env`.
    pub fn from_process() -> std::io::Result<RegistryEnv> {
        let vars = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect();
        Ok(RegistryEnv::new(vars, std::env::current_dir()?))
    }
    fn var(&self, key: &str) -> Option<&str> {
        self.vars
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
    fn home(&self) -> Option<&str> {
        self.var("HOME").or_else(|| self.var("USERPROFILE"))
    }
}

/// The device-global data directory: `$XDG_DATA_HOME`/`%LOCALAPPDATA%`/`~/.local/share` +
/// `txtodo`, falling back to the cwd when none of those resolve (mirrors `config_path`'s own
/// last-resort fallback). No override check here — callers needing `$TXTODO_REGISTRY_DB`/
/// `$TXTODO_SOCKET` check those themselves first, since each has its own override variable.
fn data_dir(env: &RegistryEnv) -> PathBuf {
    let base = env
        .var("XDG_DATA_HOME")
        .or_else(|| env.var("LOCALAPPDATA"))
        .map(PathBuf::from)
        .or_else(|| env.home().map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| env.cwd.clone());
    base.join("txtodo")
}

/// The registry database's path: `$TXTODO_REGISTRY_DB` if set (an escape hatch for tests, and a
/// future override flag, matching `$TXTODO_CONFIG`'s role for the config file); else
/// `data_dir(env)/registry.db`.
pub fn registry_db_path(env: &RegistryEnv) -> PathBuf {
    if let Some(p) = env.var("TXTODO_REGISTRY_DB") {
        return PathBuf::from(p);
    }
    data_dir(env).join("registry.db")
}

/// `registry_db_path`, but with a `--dir <workspace>`-mode fallback (task `daemon-global-socket`):
/// `$TXTODO_REGISTRY_DB` first, same as always; else, when `legacy_dir` is `Some`,
/// `<dir>/.txtodo/registry.db` — workspace-local, not the device-global default. Deliberate: every
/// existing test spawns an ephemeral tmpdir per daemon and must stay hermetic, never touching this
/// machine's real `$XDG_DATA_HOME/txtodo/registry.db`; a production `--dir` invocation that wants
/// the real shared registry can still get it via the `$TXTODO_REGISTRY_DB` override. `None`
/// resolves the true global default, same as `registry_db_path`.
pub fn registry_db_path_for(env: &RegistryEnv, legacy_dir: Option<&Path>) -> PathBuf {
    if let Some(p) = env.var("TXTODO_REGISTRY_DB") {
        return PathBuf::from(p);
    }
    match legacy_dir {
        Some(dir) => dir.join(".txtodo").join("registry.db"),
        None => data_dir(env).join("registry.db"),
    }
}

/// The one global socket's path: `$TXTODO_SOCKET` if set; else, when `legacy_dir` is `Some`, the
/// pre-existing per-workspace location (`<dir>/.txtodo/txtodod.sock`, ADR 0010) — every existing
/// test/harness that expects a `--dir`-started daemon there keeps working, unmodified; else the
/// true device-global default, `data_dir(env)/txtodod.sock`.
pub fn global_socket_path(env: &RegistryEnv, legacy_dir: Option<&Path>) -> PathBuf {
    if let Some(p) = env.var("TXTODO_SOCKET") {
        return PathBuf::from(p);
    }
    match legacy_dir {
        Some(dir) => dir.join(".txtodo").join("txtodod.sock"),
        None => data_dir(env).join("txtodod.sock"),
    }
}

/// The true global mode's state directory: wherever `global_socket_path(env, None)` actually
/// resolved to (so `$TXTODO_SOCKET` relocates the pid lock and logs right along with the socket,
/// not just the socket) — falls back to `data_dir(env)` in the unreachable case a socket path has
/// no parent. Found by a real bug: `global_pid_path`/`global_log_dir` used to call `data_dir(env)`
/// directly, ignoring `$TXTODO_SOCKET`/`$TXTODO_REGISTRY_DB` entirely, so two daemons started with
/// isolated `$TXTODO_SOCKET` overrides (e.g. two tests running concurrently) still collided on the
/// *same* real `$XDG_DATA_HOME/txtodo/txtodod.pid` — the second always lost the pid lock race and
/// refused to start with "already running", even though its socket/registry were fully isolated.
/// `pub` (task `daemon-paths-shared-crate`): `apps/desktop` needs this same directory (its own
/// client-side spawn-lock parent and log location), not just the pid/log paths built from it.
pub fn global_state_dir(env: &RegistryEnv) -> PathBuf {
    global_socket_path(env, None)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir(env))
}

/// The true global mode's pid lock path (`--dir` mode keeps using `<dir>/.txtodo/txtodod.pid`,
/// computed directly in `main.rs`, unchanged).
pub fn global_pid_path(env: &RegistryEnv) -> PathBuf {
    global_state_dir(env).join("txtodod.pid")
}

/// The true global mode's log directory (`--dir` mode keeps using `<dir>/.txtodo/logs`, computed
/// directly in `main.rs`, unchanged).
pub fn global_log_dir(env: &RegistryEnv) -> PathBuf {
    global_state_dir(env).join("logs")
}

/// The default workspace's directory (task `default-workspace`, decision A): `$TXTODO_DEFAULT_WORKSPACE`
/// if set, an escape hatch for tests and for anyone who wants it elsewhere; else `default` beside
/// the registry, in the OS's own data dir with the rest of txtodo's state: `$XDG_DATA_HOME` or
/// `~/.local/share` on macOS and Linux, `%LOCALAPPDATA%` on Windows (the same chain `data_dir`
/// already resolves for `registry.db`).
///
/// Not a path anyone syncs: devices agree on the workspace's identity, never on where it lives.
pub fn default_workspace_dir(env: &RegistryEnv) -> PathBuf {
    if let Some(p) = env.var("TXTODO_DEFAULT_WORKSPACE") {
        return PathBuf::from(p);
    }
    data_dir(env).join("default")
}

/// `default_workspace_dir`, but relocated with an isolated daemon: when `$TXTODO_SOCKET` moves the
/// daemon's state (every test and harness that isolates a daemon sets it) the default lives beside
/// that socket, never in the real `$XDG_DATA_HOME/txtodo/default`. Without this a hermetic test
/// daemon would create and register the developer's real per-user default workspace.
/// `$TXTODO_DEFAULT_WORKSPACE` still wins over both.
pub fn default_workspace_dir_for(env: &RegistryEnv) -> PathBuf {
    if let Some(p) = env.var("TXTODO_DEFAULT_WORKSPACE") {
        return PathBuf::from(p);
    }
    global_state_dir(env).join("default")
}

/// The workspace root a client should name to the daemon when the user gave no `--dir`: the
/// nearest ancestor of `start` (itself included) that already holds a `.txtodo/` directory, else
/// `start` unchanged (a first run in a fresh directory still registers that directory).
///
/// Why: a `Path` selector auto-registers whatever directory it names
/// (`txtodo-daemon`'s `WorkspaceCatalog::resolve`). A client that names its raw cwd therefore
/// registers `tasks/<slug>/`, `apps/desktop/` and every git worktree as a workspace of its own,
/// and once 2+ are open every selector-less call is "ambiguous".
///
/// The walk stops after a directory holding `.git` (a directory in a clone, a file in a linked
/// worktree): a worktree is its own checkout, not a sub-directory of the clone that contains it.
/// Same idea as how git finds its own root: <https://git-scm.com/docs/git-rev-parse#Documentation/git-rev-parse.txt---show-toplevel>
pub fn workspace_root_from(start: &Path) -> PathBuf {
    for dir in start.ancestors() {
        if dir.join(".txtodo").is_dir() {
            return dir.to_path_buf();
        }
        // `Path::exists` follows symlinks and is true for both a `.git` dir and a `.git` file.
        // Ref: https://doc.rust-lang.org/std/path/struct.Path.html#method.exists
        if dir.join(".git").exists() {
            break;
        }
    }
    start.to_path_buf()
}

/// Which workspace a client with no `--dir` means (task `default-workspace`, decided 2026-09-20):
/// the current folder when it is a workspace, else the user's default workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceChoice {
    /// The folder the client runs in (or the workspace root above it).
    Here(PathBuf),
    /// No workspace here: the default workspace's directory. A client says so out loud.
    Default(PathBuf),
}

impl WorkspaceChoice {
    /// The directory to name to the daemon (or to work in directly).
    pub fn path(&self) -> &Path {
        match self {
            WorkspaceChoice::Here(p) | WorkspaceChoice::Default(p) => p,
        }
    }

    /// True when the client fell back to the default workspace.
    pub fn is_default(&self) -> bool {
        matches!(self, WorkspaceChoice::Default(_))
    }
}

/// Whether `dir` is a workspace: the daemon has kept state there (`.txtodo/`), or it is a plain
/// todo.txt folder (`todo.txt` beside the client, the todo.sh habit). One definition for the CLI,
/// TUI and MCP, so the same folder is never a workspace to one and not to another.
pub fn is_workspace_dir(dir: &Path) -> bool {
    dir.join(".txtodo").is_dir() || dir.join("todo.txt").is_file()
}

/// The workspace a client should use when given no `--dir`: `start`'s workspace root (see
/// [`workspace_root_from`]) when that is a workspace, else the default workspace
/// ([`default_workspace_dir_for`], which follows an isolated daemon's `$TXTODO_SOCKET`).
pub fn choose_workspace(env: &RegistryEnv, start: &Path) -> WorkspaceChoice {
    let root = workspace_root_from(start);
    if is_workspace_dir(&root) {
        WorkspaceChoice::Here(root)
    } else {
        WorkspaceChoice::Default(default_workspace_dir_for(env))
    }
}

#[cfg(test)]
mod tests;
