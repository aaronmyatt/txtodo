//! Advisory-only nudge (never blocks anything) toward `txtodo skill install`, mirrored in this
//! crate's own status/health view (root todo.txt, `tasks/agent-skill-install/todo.txt`) alongside
//! `txtodo-cli`'s `doctor` skill row (`crates/txtodo-cli/src/commands/doctor.rs`'s `skill_check`).
//! This crate's `allowedDeps` (`.claude/budgets.json`) has no edge to `txtodo-cli`, so the same
//! one-file existence check is duplicated here rather than shared.

use std::path::{Path, PathBuf};

/// True when no prior `txtodo skill install` has written the Claude Code Skill target.
pub fn needed(home: Option<&Path>) -> bool {
    !home.is_some_and(|h| h.join(".claude/skills/txtodo-backlog/SKILL.md").exists())
}

/// Resolves `$HOME` (or `%USERPROFILE%` on Windows), the same fallback order as `txtodo-cli`'s
/// `commands::skill::home_dir`.
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
        assert!(needed(None));
    }

    #[test]
    fn needed_when_skill_md_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(needed(Some(dir.path())));
    }

    #[test]
    fn not_needed_once_skill_md_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join(".claude/skills/txtodo-backlog");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(skill_dir.join("SKILL.md"), "playbook").expect("write");
        assert!(!needed(Some(dir.path())));
    }
}
