//! Which `txtodod` a client launches or writes into a service unit, and which socket the boot unit
//! owns — split out of `spawn.rs` for its file budget.
//!
//! The `#[cfg(any(unix, test))]` helpers are called only from `spawn.rs`'s `#[cfg(unix)] mod
//! unix_impl`, so a Windows lib build would flag them `dead_code` (an error under CI's clippy
//! `-D warnings`). `test` keeps them compiled for this file's tests on every platform.
//! Ref: https://doc.rust-lang.org/reference/conditional-compilation.html#the-cfg-attribute

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The `txtodod` to launch: an explicit override; else a real (non-symlink) `txtodod` beside the
/// running executable — `just install-daemon` puts `txtodo` and `txtodod` in one directory, and
/// `txtodo daemon install` already records that sibling; else the first one on `$PATH`.
///
/// The `$PATH` copy alone is unsafe: it can be a symlink into a checkout's `target/release`, so a
/// rebuild swaps the binary under a running daemon, and an ad-hoc spawn or a repaired unit points
/// at it (root todo id:01M2WX72DCYZK18VFRCX4YC5Y1).
#[cfg(any(unix, test))]
pub(crate) fn resolve_binary(daemon_bin: Option<&PathBuf>) -> Option<PathBuf> {
    if let Some(bin) = daemon_bin {
        return Some(bin.clone());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| sibling_txtodod(&exe))
        .or_else(|| which_on_path("txtodod"))
}

/// A real `txtodod` file next to `exe`, if there is one. Not a symlink (its target may be a build
/// directory), and never inside a macOS `.app` bundle (an updater replaces that binary).
#[cfg(any(unix, test))]
pub(crate) fn sibling_txtodod(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let in_bundle = dir
        .components()
        .any(|c| c.as_os_str().to_string_lossy().ends_with(".app"));
    if in_bundle {
        return None;
    }
    let candidate = dir.join("txtodod");
    // `symlink_metadata` does not follow the link: a symlink reads as not a file.
    candidate
        .symlink_metadata()
        .ok()
        .filter(|m| m.is_file())
        .map(|_| candidate)
}

/// The same `$PATH` search a bare `Command::new("txtodod")` performs, needed here because the
/// rendered service file wants a concrete path, not a bare program name.
#[cfg(any(unix, test))]
fn which_on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(program))
            .find(|p| p.is_file())
    })
}

/// The socket the boot-time unit's daemon binds: the device-global default,
/// `$XDG_DATA_HOME/txtodo/txtodod.sock` else `~/.local/share/txtodo/txtodod.sock`. Must match
/// `txtodo_workspace_paths::global_socket_path` with no `$TXTODO_SOCKET` set — this crate may not
/// depend on that one (`.claude/budgets.json`'s `allowedDeps`), so `txtodo-cli`'s tests pin the two
/// together.
pub fn default_global_socket(home: &Path, xdg_data_home: Option<&OsStr>) -> PathBuf {
    let base = xdg_data_home
        .filter(|v| !v.is_empty())
        .map_or_else(|| home.join(".local/share"), PathBuf::from);
    base.join("txtodo").join("txtodod.sock")
}

/// Whether a client dialing `socket` is talking to the daemon the boot unit runs. A caller that
/// isolates itself with `TXTODO_SOCKET` or `XDG_DATA_HOME` (a test, a second install) dials some
/// other socket, and a unit installed against the real `$HOME` says nothing about it — waiting for
/// that unit would wait on a socket nothing will bind (root todo id:01M2WK5DQQ500JATEN1C410KWG).
#[cfg(any(unix, test))]
pub(crate) fn unit_owns_socket(socket: &Path, home: &Path, xdg_data_home: Option<&OsStr>) -> bool {
    socket == default_global_socket(home, xdg_data_home)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::write(p, b"#!/bin/sh\n").unwrap_or_else(|e| panic!("write {}: {e}", p.display()));
    }

    #[test]
    fn a_real_txtodod_beside_the_executable_is_preferred() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        touch(&dir.path().join("txtodod"));
        assert_eq!(
            sibling_txtodod(&dir.path().join("txtodo")),
            Some(dir.path().join("txtodod"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_sibling_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let build = dir.path().join("target-release-txtodod");
        touch(&build);
        std::os::unix::fs::symlink(&build, dir.path().join("txtodod"))
            .unwrap_or_else(|e| panic!("symlink: {e}"));
        assert_eq!(sibling_txtodod(&dir.path().join("txtodo")), None);
    }

    #[test]
    fn a_missing_sibling_and_an_app_bundle_are_both_none() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        assert_eq!(sibling_txtodod(&dir.path().join("txtodo")), None);
        let bundle = dir.path().join("txtodo.app/Contents/MacOS");
        std::fs::create_dir_all(&bundle).unwrap_or_else(|e| panic!("mkdir: {e}"));
        touch(&bundle.join("txtodod"));
        assert_eq!(sibling_txtodod(&bundle.join("txtodo")), None);
    }

    #[test]
    fn an_explicit_override_wins() {
        let bin = PathBuf::from("/opt/custom/txtodod");
        assert_eq!(resolve_binary(Some(&bin)), Some(bin));
    }

    #[test]
    fn the_default_socket_follows_xdg_then_home() {
        let home = Path::new("/home/a");
        assert_eq!(
            default_global_socket(home, None),
            Path::new("/home/a/.local/share/txtodo/txtodod.sock")
        );
        assert_eq!(
            default_global_socket(home, Some(OsStr::new("/xdg"))),
            Path::new("/xdg/txtodo/txtodod.sock")
        );
        assert_eq!(
            default_global_socket(home, Some(OsStr::new(""))),
            Path::new("/home/a/.local/share/txtodo/txtodod.sock"),
            "an empty XDG_DATA_HOME is unset"
        );
    }

    #[test]
    fn a_process_env_isolated_caller_is_not_owned_by_the_unit() {
        let home = Path::new("/home/a");
        let real = Path::new("/home/a/.local/share/txtodo/txtodod.sock");
        assert!(unit_owns_socket(real, home, None));
        assert!(
            !unit_owns_socket(Path::new("/tmp/test-x/txtodod.sock"), home, None),
            "TXTODO_SOCKET pointing elsewhere"
        );
        assert!(
            !unit_owns_socket(real, home, Some(OsStr::new("/tmp/xdg"))),
            "XDG_DATA_HOME moved the data dir"
        );
    }
}
