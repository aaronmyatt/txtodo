//! Daemon connectivity state surfaced to the Svelte frontend (design §5): daemon-absent or
//! disconnected renders as a reconnect banner, never a crash.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Connectivity state of the bridge to `txtodod`. Queryable via the `daemon_status` command and
/// pushed as a `daemon-status` event on every change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonStatus {
    /// A live connection answered `Health`.
    Connected,
    /// A connect attempt is in flight.
    Connecting,
    /// `ensure_daemon` is spawning `txtodod`.
    Spawning,
    /// Every retry failed; the frontend shows the reconnect banner with its retry button.
    Dead,
}

/// Advisory-only nudge (never blocks anything) toward `txtodo skill install`, surfaced by the
/// `skill_hint` command as an onboarding banner (root todo.txt `agent-skill-install`,
/// `tasks/agent-skill-install/todo.txt` id:01M2HTCV56RRB7ETP8GG17PVN1). Mirrors `txtodo-cli`'s
/// `doctor` skill row and `crates/txtodo-tui/src/skill_hint.rs`'s status-line hint — this crate
/// has no Cargo dependency on either, so the one-file `SKILL.md` existence check is duplicated
/// here too, not shared.
pub fn skill_hint_needed(home: Option<&Path>) -> bool {
    !home.is_some_and(|h| h.join(".claude/skills/txtodo-backlog/SKILL.md").exists())
}

/// Resolves `$HOME` (or `%USERPROFILE%` on Windows), the same fallback order as
/// `crates/txtodo-tui/src/skill_hint.rs::home_dir` and `txtodo-cli`'s `commands::skill::home_dir`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needed_when_home_missing() {
        assert!(skill_hint_needed(None));
    }

    #[test]
    fn needed_when_skill_md_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(skill_hint_needed(Some(dir.path())));
    }

    #[test]
    fn not_needed_once_skill_md_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join(".claude/skills/txtodo-backlog");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(skill_dir.join("SKILL.md"), "playbook").expect("write");
        assert!(!skill_hint_needed(Some(dir.path())));
    }
}
