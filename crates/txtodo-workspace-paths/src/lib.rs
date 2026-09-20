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

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> RegistryEnv {
        RegistryEnv::new(
            vars.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            PathBuf::from("/cwd"),
        )
    }

    #[test]
    fn explicit_override_wins() {
        let e = env(&[("TXTODO_REGISTRY_DB", "/custom/registry.db")]);
        assert_eq!(registry_db_path(&e), PathBuf::from("/custom/registry.db"));
    }

    #[test]
    fn xdg_data_home_is_preferred_over_home_fallback() {
        let e = env(&[("XDG_DATA_HOME", "/xdg-data"), ("HOME", "/home/a")]);
        assert_eq!(
            registry_db_path(&e),
            PathBuf::from("/xdg-data/txtodo/registry.db")
        );
    }

    #[test]
    fn falls_back_to_home_dot_local_share() {
        let e = env(&[("HOME", "/home/a")]);
        assert_eq!(
            registry_db_path(&e),
            PathBuf::from("/home/a/.local/share/txtodo/registry.db")
        );
    }

    #[test]
    fn falls_back_to_cwd_when_nothing_resolves() {
        let e = env(&[]);
        assert_eq!(
            registry_db_path(&e),
            PathBuf::from("/cwd/txtodo/registry.db")
        );
    }

    #[test]
    fn registry_db_path_for_prefers_override_then_legacy_dir_then_global_default() {
        let dir = PathBuf::from("/workspace");
        let e = env(&[("TXTODO_REGISTRY_DB", "/custom/registry.db")]);
        assert_eq!(
            registry_db_path_for(&e, Some(&dir)),
            PathBuf::from("/custom/registry.db"),
            "override wins even over a legacy dir"
        );
        let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
        assert_eq!(
            registry_db_path_for(&e, Some(&dir)),
            PathBuf::from("/workspace/.txtodo/registry.db"),
            "legacy dir wins over the global default when no override is set"
        );
        assert_eq!(
            registry_db_path_for(&e, None),
            PathBuf::from("/xdg-data/txtodo/registry.db"),
            "no legacy dir falls back to the true global default"
        );
    }

    #[test]
    fn global_socket_path_prefers_override_then_legacy_dir_then_global_default() {
        let dir = PathBuf::from("/workspace");
        let e = env(&[("TXTODO_SOCKET", "/custom/txtodod.sock")]);
        assert_eq!(
            global_socket_path(&e, Some(&dir)),
            PathBuf::from("/custom/txtodod.sock")
        );
        let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
        assert_eq!(
            global_socket_path(&e, Some(&dir)),
            PathBuf::from("/workspace/.txtodo/txtodod.sock"),
            "the pre-existing per-workspace socket location, unchanged"
        );
        assert_eq!(
            global_socket_path(&e, None),
            PathBuf::from("/xdg-data/txtodo/txtodod.sock")
        );
    }

    #[test]
    fn global_pid_and_log_paths_sit_beside_the_registry() {
        let e = env(&[("XDG_DATA_HOME", "/xdg-data")]);
        assert_eq!(
            global_pid_path(&e),
            PathBuf::from("/xdg-data/txtodo/txtodod.pid")
        );
        assert_eq!(global_log_dir(&e), PathBuf::from("/xdg-data/txtodo/logs"));
    }

    /// Regression test for a real bug caught running the `global_socket` integration test
    /// concurrently: `global_pid_path`/`global_log_dir` used to call `data_dir(env)` directly,
    /// ignoring `$TXTODO_SOCKET` — so two daemons started with isolated socket overrides (e.g. two
    /// tests, or two tempdir-scoped harnesses on one real machine) still fought over the *same*
    /// real `$XDG_DATA_HOME/txtodo/txtodod.pid`, and the loser refused to start with "already
    /// running" even though nothing it actually owned (socket, registry) was shared.
    #[test]
    fn global_pid_and_log_paths_follow_the_socket_override_not_just_xdg_data_home() {
        let e = env(&[
            ("XDG_DATA_HOME", "/xdg-data"),
            ("TXTODO_SOCKET", "/tmp/isolated-a/txtodod.sock"),
        ]);
        assert_eq!(
            global_pid_path(&e),
            PathBuf::from("/tmp/isolated-a/txtodod.pid")
        );
        assert_eq!(global_log_dir(&e), PathBuf::from("/tmp/isolated-a/logs"));
    }

    /// A fresh directory tree under the OS temp dir (this crate has no dependencies, so no
    /// `tempfile`). `mk` lists directories to create, relative to the returned root.
    fn tree(name: &str, mk: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("txtodo-wp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in mk {
            std::fs::create_dir_all(root.join(d)).expect("create test dir");
        }
        root
    }

    #[test]
    fn subdirectory_of_a_workspace_resolves_up_to_its_root() {
        let root = tree("up", &[".txtodo", "tasks/slug"]);
        assert_eq!(workspace_root_from(&root.join("tasks/slug")), root);
        assert_eq!(workspace_root_from(&root), root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_directory_with_no_workspace_above_it_stays_as_named() {
        let root = tree("fresh", &["a/b"]);
        assert_eq!(workspace_root_from(&root.join("a/b")), root.join("a/b"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A linked git worktree carries `.git` (a file there, a dir here: both count) and must not be
    /// folded into the clone that happens to contain it.
    #[test]
    fn a_git_boundary_stops_the_walk() {
        let root = tree("git", &[".txtodo", "wt/.git", "wt/sub"]);
        assert_eq!(
            workspace_root_from(&root.join("wt/sub")),
            root.join("wt/sub")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
