//! `txtodo conflicts [list|delete|keep-newest]`: a file's duplicate lines (ADR 0032), two or more
//! tasks whose whole lines are the same bytes. Every subcommand needs the daemon, which derives the
//! groups from the file it holds. Same-word review (`needs_review` flags and `conflicts resolve`)
//! went with the todo.txt mirror (ADR 0038).

use crate::client::Daemon;
use crate::{CliError, Ctx};
use clap::Subcommand;
use txtodo_core::parse_file;
use txtodo_proto::v1 as pb;

/// The `txtodo conflicts` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// The file's duplicate groups, one block per group.
    #[command(visible_alias = "ls")]
    List {
        /// Which document (workspace-relative); the workspace's root list by default.
        #[arg(long)]
        file: Option<String>,
    },
    /// Delete one copy of a duplicated line (ADR 0032), after a confirm.
    Delete {
        /// The line the copy sits on now; it must be in a duplicate group.
        line: u32,
        /// Which document (workspace-relative); the workspace's root list by default.
        #[arg(long)]
        file: Option<String>,
        /// Skip the confirm.
        #[arg(long)]
        yes: bool,
    },
    /// In every duplicate group, keep the copy with the newest id and delete the rest (one
    /// confirm, one write).
    KeepNewest {
        /// Which document (workspace-relative); the workspace's root list by default.
        #[arg(long)]
        file: Option<String>,
        /// Skip the confirm.
        #[arg(long)]
        yes: bool,
    },
}

/// Entry: `txtodo conflicts` with no subcommand is `list` (notes: an alias for muscle memory).
/// `--file` defaults to the workspace's root list (`Paths::todo_file`, task layout-client-gaps),
/// the same document every other daemon-mode command reads — not the literal `todo.txt`.
pub fn run(
    ctx: &Ctx,
    daemon: &mut Daemon,
    action: Option<&Action>,
    as_json: bool,
) -> Result<(), CliError> {
    let root = ctx.paths.todo_file.as_str();
    match action {
        None => run_list(daemon, root, as_json),
        Some(Action::List { file }) => run_list(daemon, file.as_deref().unwrap_or(root), as_json),
        Some(Action::Delete { line, file, yes }) => {
            dup::run_delete(daemon, file.as_deref().unwrap_or(root), *line, *yes)
        }
        Some(Action::KeepNewest { file, yes }) => {
            dup::run_keep_newest(daemon, file.as_deref().unwrap_or(root), *yes)
        }
    }
}

/// `conflicts list`: the file's duplicate groups (ADR 0032), or the todo.sh-style "nothing to
/// report" line.
pub fn run_list(daemon: &mut Daemon, file: &str, as_json: bool) -> Result<(), CliError> {
    let pb::ConflictsResponse { duplicates, .. } = daemon.conflicts(file)?;
    if duplicates.is_empty() {
        // JSON mode prints nothing: absence of lines means "none", and scripts grep objects.
        if !as_json {
            println!("TODO: no conflicts.");
        }
        return Ok(());
    }
    let lines = current_lines(daemon, file)?;
    dup::print_groups(&duplicates, &lines, as_json);
    Ok(())
}

/// The file's lines as they are now, for each group's copies.
fn current_lines(daemon: &mut Daemon, file: &str) -> Result<Vec<String>, CliError> {
    let bytes = daemon.get(file)?;
    Ok(parse_file(&bytes)
        .lines
        .iter()
        // An opaque (non-UTF-8) line cannot be a task; show it for what it is.
        .map(|l| l.raw().unwrap_or("(not UTF-8)").to_owned())
        .collect())
}

// Duplicate lines (ADR 0032), split out for this file's budget.
#[path = "conflicts_dup.rs"]
mod dup;
