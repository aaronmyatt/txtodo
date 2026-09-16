//! Where the device-global `txtodod` listens, for `main.rs`'s `--global` mode (mcp-multi-
//! workspace-gateway): reachable at one socket regardless of which workspace(s) it has open,
//! unlike the pre-existing `--dir` bridge (`grpc_backend.rs::SOCKET_REL`), which only ever reaches
//! whatever single directory's own daemon a caller started. Reimplements the same resolution
//! `txtodo-daemon`'s `workspace_registry_paths::global_socket_path(env, None)` uses rather than
//! importing it — this crate may not depend on `txtodo-daemon` (the dependency runs the other way:
//! the daemon depends on this crate, `budgets.json`'s `allowedDeps`), the same reasoning
//! `grpc_backend.rs`'s `SOCKET_REL` doc already gives for reimplementing the per-workspace socket
//! path instead of importing `txtodo-cli`. Kept deliberately small: only the pieces `--global`
//! mode needs (the socket and log directory), not the daemon's own registry/pid-lock paths.

use std::path::PathBuf;

/// `$TXTODO_SOCKET` if set; else the platform data directory (`$XDG_DATA_HOME`/`%LOCALAPPDATA%`/
/// `~/.local/share`, falling back to the cwd when none resolve) + `txtodo/txtodod.sock` — mirrors
/// `workspace_registry_paths::global_socket_path(env, None)`'s own fallback chain exactly.
pub fn path() -> PathBuf {
    path_from(&var)
}

/// The log directory sitting beside [`path`]'s socket — mirrors
/// `workspace_registry_paths::global_log_dir`, which derives from the (possibly `$TXTODO_SOCKET`-
/// overridden) socket path's parent rather than `data_dir()` directly, so an isolated
/// `$TXTODO_SOCKET` override relocates logs right along with it (the same bug that module's own
/// doc flags fixing).
pub fn log_dir() -> PathBuf {
    log_dir_from(&var)
}

fn var(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Testable core: `lookup` stands in for `std::env::var` so tests never need `unsafe` process-
/// global env mutation (`#![forbid(unsafe_code)]`, this crate-wide) — a plain closure over an
/// in-memory map instead, the same "inject the environment" idiom
/// `workspace_registry_paths::RegistryEnv` uses on the daemon side.
fn path_from(lookup: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(p) = lookup("TXTODO_SOCKET") {
        return PathBuf::from(p);
    }
    data_dir_from(lookup).join("txtodod.sock")
}

fn log_dir_from(lookup: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    path_from(lookup)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| data_dir_from(lookup))
        .join("logs")
}

fn data_dir_from(lookup: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let base = lookup("XDG_DATA_HOME")
        .or_else(|| lookup("LOCALAPPDATA"))
        .map(PathBuf::from)
        .or_else(|| lookup("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    base.join("txtodo")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<&str, &str> = vars.iter().copied().collect();
        move |k| map.get(k).map(|v| (*v).to_owned())
    }

    #[test]
    fn explicit_override_wins() {
        let l = lookup(&[("TXTODO_SOCKET", "/custom/txtodod.sock")]);
        assert_eq!(path_from(&l), PathBuf::from("/custom/txtodod.sock"));
    }

    #[test]
    fn falls_back_to_xdg_data_home() {
        let l = lookup(&[("XDG_DATA_HOME", "/xdg-data")]);
        assert_eq!(
            path_from(&l),
            PathBuf::from("/xdg-data/txtodo/txtodod.sock")
        );
        assert_eq!(log_dir_from(&l), PathBuf::from("/xdg-data/txtodo/logs"));
    }

    #[test]
    fn log_dir_follows_the_socket_override_not_just_xdg_data_home() {
        let l = lookup(&[
            ("XDG_DATA_HOME", "/xdg-data"),
            ("TXTODO_SOCKET", "/tmp/isolated-a/txtodod.sock"),
        ]);
        assert_eq!(log_dir_from(&l), PathBuf::from("/tmp/isolated-a/logs"));
    }
}
