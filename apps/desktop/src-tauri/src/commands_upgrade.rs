//! Settings > General's "Updates" row (task self-upgrade). The app holds no upgrade logic: it runs
//! the `txtodo` CLI (`txtodo upgrade --check --json`, then `txtodo upgrade`), which knows how this
//! device was installed, and offers a relaunch so the new app build is the one on screen. The app
//! bundles only `txtodod`, so the CLI is looked for where the install paths put it: beside this
//! executable (a dev checkout), then the install script's, Homebrew's and `just install`'s
//! folders — a GUI app on macOS does not get the shell's `$PATH`.
//! Ref: https://doc.rust-lang.org/std/process/struct.Command.html

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::AppHandle;

/// What `txtodo upgrade --check --json` said. Mirrors `txtodo-cli`'s `check_json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateCheckDto {
    /// This build's version.
    pub current: String,
    /// The newest release tag.
    pub latest: String,
    /// Whether `latest` is newer than this build.
    pub newer: bool,
    /// What the upgrade runs.
    pub command: String,
}

/// Where a `txtodo` may be, in order, for `exe` (this app's executable) and `home`.
fn candidates(exe: &Path, home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = exe.parent() {
        out.push(dir.join("txtodo"));
    }
    out.push(home.join(".local/bin/txtodo"));
    out.push(PathBuf::from("/opt/homebrew/bin/txtodo"));
    out.push(PathBuf::from("/usr/local/bin/txtodo"));
    out.push(home.join(".cargo/bin/txtodo"));
    out
}

/// The first `txtodo` that exists, else an error that says how to get one.
fn txtodo_bin() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    candidates(&exe, &home)
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            "no txtodo CLI found: install it with the install script (README), then check again"
                .to_owned()
        })
}

/// The first line of a failed run's stderr, for the row.
fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("the command failed")
        .to_owned()
}

/// `txtodo upgrade --check --json`. Off the async runtime's threads: it waits on the network.
#[tracing::instrument(name = "ipc.check_updates", skip_all)]
#[tauri::command]
pub async fn check_updates() -> Result<UpdateCheckDto, String> {
    let bin = txtodo_bin()?;
    let out = tauri::async_runtime::spawn_blocking(move || {
        Command::new(bin)
            .args(["upgrade", "--check", "--json"])
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(first_line(&out.stderr));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("txtodo upgrade said: {e}"))
}

/// `txtodo upgrade`: the release for this install, then the daemon restart. Returns its last line.
#[tracing::instrument(name = "ipc.run_upgrade", skip_all)]
#[tauri::command]
pub async fn run_upgrade() -> Result<String, String> {
    let bin = txtodo_bin()?;
    let out =
        tauri::async_runtime::spawn_blocking(move || Command::new(bin).arg("upgrade").output())
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(first_line(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap_or("upgraded")
        .to_owned())
}

/// Starts the app again, so the upgraded build is the one on screen.
/// Ref: https://docs.rs/tauri/2/tauri/struct.AppHandle.html#method.restart
#[tauri::command]
pub fn relaunch(app: AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_is_looked_for_beside_the_app_then_in_the_install_folders() {
        let found = candidates(Path::new("/x/target/debug/desktop"), Path::new("/Users/me"));
        assert_eq!(found[0], PathBuf::from("/x/target/debug/txtodo"));
        assert_eq!(found[1], PathBuf::from("/Users/me/.local/bin/txtodo"));
        assert!(found.contains(&PathBuf::from("/opt/homebrew/bin/txtodo")));
        assert_eq!(
            found.last(),
            Some(&PathBuf::from("/Users/me/.cargo/bin/txtodo"))
        );
    }

    #[test]
    fn the_check_reads_the_clis_json() {
        let json =
            r#"{"current":"0.0.22","latest":"v0.0.23","newer":true,"command":"brew upgrade x"}"#;
        let dto: UpdateCheckDto = serde_json::from_str(json).unwrap_or_else(|e| panic!("{e}"));
        assert!(dto.newer);
        assert_eq!(dto.latest, "v0.0.23");
    }

    #[test]
    fn a_failure_shows_its_first_words() {
        assert_eq!(first_line(b"\ntxtodo: no curl\nmore"), "txtodo: no curl");
        assert_eq!(first_line(b""), "the command failed");
    }
}
