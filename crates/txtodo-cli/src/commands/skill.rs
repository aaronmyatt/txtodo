//! `txtodo skill install`: drops the agent playbook for working this backlog onto disk — a Claude
//! Code Skill under the user's global `~/.claude/skills`
//! (https://docs.claude.com/en/docs/claude-code/skills), and a marked section in this project's
//! `AGENTS.md` (the cross-agent convention several coding agents already read at a project root).
//! Both targets render the same canonical playbook (`skills/txtodo-backlog.md`, embedded at
//! compile time) so there is one wording to maintain.

use crate::CliError;
use clap::ValueEnum;
use std::fs;
use std::path::{Path, PathBuf};

/// The canonical playbook: how an agent should work a txtodo backlog end to end.
const PLAYBOOK: &str = include_str!("../../../../skills/txtodo-backlog.md");

const AGENTS_START: &str = "<!-- txtodo:skill:start -->";
const AGENTS_END: &str = "<!-- txtodo:skill:end -->";

/// Where to install the playbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Target {
    /// A Claude Code Skill under `~/.claude/skills` (global: every project on this machine).
    Claude,
    /// A marked section in this project's `AGENTS.md` (the cross-agent convention).
    Agents,
}

/// `txtodo skill install [--only claude|agents]`.
#[derive(Debug, clap::Subcommand)]
pub enum Action {
    /// Writes the playbook to every target (or just `--only` one).
    Install {
        /// Install only this target; default installs both.
        #[arg(long, value_enum)]
        only: Option<Target>,
    },
}

/// Dispatches `txtodo skill`'s one action.
pub fn run(action: &Action) -> Result<(), CliError> {
    let Action::Install { only } = action;
    let targets = match only {
        Some(t) => vec![*t],
        None => vec![Target::Claude, Target::Agents],
    };
    for target in targets {
        match target {
            Target::Claude => install_claude()?,
            Target::Agents => install_agents_at(Path::new("AGENTS.md"))?,
        }
    }
    Ok(())
}

/// Whether a prior `install` already wrote the Claude target — `txtodo doctor`'s advisory hint.
pub fn claude_installed() -> bool {
    home_dir().is_some_and(|h| skill_md_path(&h).exists())
}

fn skill_md_path(home: &Path) -> PathBuf {
    home.join(".claude/skills/txtodo-backlog/SKILL.md")
}

fn install_claude() -> Result<(), CliError> {
    let home = home_dir()
        .ok_or_else(|| CliError::Message("txtodo: no home directory (set $HOME)".to_owned()))?;
    let path = skill_md_path(&home);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let doc = format!(
        "---\nname: txtodo-backlog\ndescription: Work a txtodo backlog (todo.txt + tasks/*) end \
         to end via the txtodo MCP tools — picking, claiming and closing out tasks.\n---\n\n{PLAYBOOK}"
    );
    fs::write(&path, doc)?;
    println!("wrote {}", path.display());
    Ok(())
}

fn install_agents_at(path: &Path) -> Result<(), CliError> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let block = format!("{AGENTS_START}\n\n{PLAYBOOK}\n{AGENTS_END}\n");
    fs::write(path, merge_block(&existing, &block))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Replaces the marked block if present (idempotent re-install: the trailing newline `block`
/// always writes is swallowed back out of the old tail so repeat installs don't grow the file),
/// else appends it.
fn merge_block(existing: &str, block: &str) -> String {
    match (existing.find(AGENTS_START), existing.find(AGENTS_END)) {
        (Some(start), Some(end)) => {
            let mut tail = end + AGENTS_END.len();
            if existing.as_bytes().get(tail) == Some(&b'\n') {
                tail += 1;
            }
            format!("{}{block}{}", &existing[..start], &existing[tail..])
        }
        _ if existing.is_empty() => block.to_owned(),
        _ => format!("{}\n\n{block}", existing.trim_end()),
    }
}

/// `$HOME`, or `$USERPROFILE` on Windows — no new dependency for one lookup.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_block_appends_when_absent() {
        let out = merge_block(
            "existing\n",
            "<!-- txtodo:skill:start -->\nX\n<!-- txtodo:skill:end -->\n",
        );
        assert!(out.starts_with("existing"));
        assert!(out.contains("X"));
    }

    #[test]
    fn merge_block_replaces_existing_block_and_is_idempotent() {
        let before = format!("keep\n\n{AGENTS_START}\nold\n{AGENTS_END}\n");
        let block = format!("{AGENTS_START}\nnew\n{AGENTS_END}\n");
        let out = merge_block(&before, &block);
        assert!(out.contains("keep"));
        assert!(out.contains("new"));
        assert!(!out.contains("old"));
        assert_eq!(
            out,
            merge_block(&out, &block),
            "reinstalling must not grow the file"
        );
    }

    #[test]
    fn install_agents_at_writes_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("AGENTS.md");
        install_agents_at(&path).expect("install");
        let text = fs::read_to_string(&path).expect("read back");
        assert!(text.contains(AGENTS_START));
        assert!(text.contains("Working a txtodo backlog"));
    }
}
