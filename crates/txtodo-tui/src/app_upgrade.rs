//! Settings > General's "Updates" row (task self-upgrade). The TUI holds no upgrade logic: it asks
//! the `txtodo` beside it (`txtodo upgrade --check --json`) and, when the human picks the upgrade,
//! quits and runs `txtodo upgrade` once the terminal is back to normal, so its output and any
//! prompt (Homebrew, the install script) reach the human.
//! Ref: <https://doc.rust-lang.org/std/process/struct.Command.html>

use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::settings_rows::{Act, SRow};
use crate::state::AppState;
use crate::state_settings::UpdateCheck;

/// Set by the "Updates" row; read once the terminal is restored ([`run_if_requested`]).
static AFTER_EXIT: AtomicBool = AtomicBool::new(false);

/// The `txtodo` beside this binary (every install puts them in one folder), else the one on
/// `$PATH`.
fn txtodo_bin() -> PathBuf {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("txtodo")))
        .filter(|p| p.is_file());
    sibling.unwrap_or_else(|| PathBuf::from("txtodo"))
}

/// The string value of `"key":"…"` in one line of JSON, `\"` and `\\` unescaped. The CLI writes
/// the object (`txtodo-cli/src/commands/upgrade.rs::check_json`); this crate has no JSON parser.
fn string_field(json: &str, key: &str) -> Option<String> {
    let start = json.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let mut out = String::new();
    let mut chars = json[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            c => out.push(c),
        }
    }
    None
}

/// What `txtodo upgrade --check --json` said.
pub fn parse(json: &str) -> UpdateCheck {
    let latest = string_field(json, "latest");
    let command = string_field(json, "command");
    match (latest, command, json.contains("\"newer\":true")) {
        (Some(latest), Some(command), true) => UpdateCheck::Available { latest, command },
        (Some(latest), _, false) => UpdateCheck::Current(latest),
        _ => UpdateCheck::Failed("txtodo upgrade gave no answer".to_owned()),
    }
}

/// Runs the check off the event loop's thread and keeps the answer for the row.
pub async fn check(state: &mut AppState) {
    let bin = txtodo_bin();
    let out = tokio::task::spawn_blocking(move || {
        Command::new(bin)
            .args(["upgrade", "--check", "--json"])
            .output()
    })
    .await;
    state.settings.update = match out {
        Ok(Ok(out)) if out.status.success() => parse(&String::from_utf8_lossy(&out.stdout)),
        Ok(Ok(out)) => UpdateCheck::Failed(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("the check failed")
                .to_owned(),
        ),
        Ok(Err(e)) => UpdateCheck::Failed(format!("cannot run txtodo: {e}")),
        Err(e) => UpdateCheck::Failed(format!("the check stopped: {e}")),
    };
}

/// The "Updates" row: Enter checks, and once a newer release is known, Enter quits and upgrades.
pub fn row(state: &AppState) -> SRow {
    let (value, act) = match &state.settings.update {
        UpdateCheck::NotChecked => (
            "Enter checks for a newer release".to_owned(),
            Act::CheckUpdates,
        ),
        UpdateCheck::Current(tag) => (
            format!("{tag} is the newest \u{b7} Enter checks again"),
            Act::CheckUpdates,
        ),
        UpdateCheck::Available { latest, command } => (
            format!("{latest} is out \u{b7} Enter quits and runs: {command}"),
            Act::Upgrade,
        ),
        UpdateCheck::Failed(why) => (format!("{why} \u{b7} Enter retries"), Act::CheckUpdates),
    };
    SRow {
        label: "Updates".to_owned(),
        value,
        act: Some(act),
        ..SRow::default()
    }
}

/// The row picked the upgrade: it runs after the TUI quits.
pub fn request_after_exit() {
    AFTER_EXIT.store(true, Ordering::SeqCst);
}

/// After the terminal is restored: runs `txtodo upgrade` in this terminal when it was asked for.
#[allow(clippy::print_stderr)] // the terminal is ours again; this is the only way to say why
pub fn run_if_requested() -> Option<ExitCode> {
    if !AFTER_EXIT.load(Ordering::SeqCst) {
        return None;
    }
    let ok = Command::new(txtodo_bin())
        .arg("upgrade")
        .status()
        .map_err(|e| eprintln!("txtodo-tui: cannot run `txtodo upgrade`: {e}"))
        .is_ok_and(|s| s.success());
    Some(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_release_names_its_tag_and_command() {
        let json = r#"{"current":"0.0.22","latest":"v0.0.23","newer":true,"command":"brew upgrade aaronmyatt/tap/txtodo"}"#;
        assert_eq!(
            parse(json),
            UpdateCheck::Available {
                latest: "v0.0.23".to_owned(),
                command: "brew upgrade aaronmyatt/tap/txtodo".to_owned()
            }
        );
    }

    #[test]
    fn the_newest_is_current_and_junk_is_a_failure() {
        let json = r#"{"current":"0.0.22","latest":"v0.0.22","newer":false,"command":"x"}"#;
        assert_eq!(parse(json), UpdateCheck::Current("v0.0.22".to_owned()));
        assert!(matches!(parse("oops"), UpdateCheck::Failed(_)));
    }

    #[test]
    fn escaped_quotes_survive() {
        assert_eq!(
            string_field(r#"{"command":"say \"hi\""}"#, "command").as_deref(),
            Some("say \"hi\"")
        );
    }
}
