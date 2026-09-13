//! Where the workspace registry's own database lives: a device-global location, deliberately
//! outside any single workspace's `<workspace>/.txtodo/` (ADR 0010 fixes that path per workspace;
//! a *device*-global catalog has no single workspace to live under). See
//! `tasks/daemon-workspace-registry/notes.md` for the full reasoning — in short, `$XDG_DATA_HOME`
//! (or the platform equivalent), not `$XDG_CONFIG_HOME`: this is generated, mutable state the
//! daemon owns, not something a human hand-edits like `config.toml`. Mirrors
//! `txtodo-cli/src/config.rs`'s `Env`/`config_path` env-injection idiom so a test never touches
//! the real machine's home directory; not itself wired into `main.rs` yet (a later task's job —
//! see this module's own doc note in `workspace_registry.rs`).

use std::collections::BTreeMap;
use std::path::PathBuf;

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

/// The registry database's path: `$TXTODO_REGISTRY_DB` if set (an escape hatch for tests, and a
/// future override flag, matching `$TXTODO_CONFIG`'s role for the config file); else
/// `$XDG_DATA_HOME`/`%LOCALAPPDATA%`/`~/.local/share` + `txtodo/registry.db`, falling back to the
/// cwd when none of those resolve (mirrors `config_path`'s own last-resort fallback).
pub fn registry_db_path(env: &RegistryEnv) -> PathBuf {
    if let Some(p) = env.var("TXTODO_REGISTRY_DB") {
        return PathBuf::from(p);
    }
    let base = env
        .var("XDG_DATA_HOME")
        .or_else(|| env.var("LOCALAPPDATA"))
        .map(PathBuf::from)
        .or_else(|| env.home().map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| env.cwd.clone());
    base.join("txtodo").join("registry.db")
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
}
