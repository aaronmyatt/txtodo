//! The commands that need no workspace, no config and no daemon, answered before `main` resolves any
//! of them: about this binary (`upgrade`), or about a git clone given on the command line
//! (`merge-driver`, `git`). Split out of `main.rs` for its line budget.

use crate::CliError;
use crate::cli::{Cli, Command};

/// The result of an early command, or `None` when `cli.command` is not one.
pub fn run(cli: &Cli) -> Option<Result<(), CliError>> {
    Some(match &cli.command {
        Command::Upgrade { check } => super::upgrade::run(*check, cli.json),
        Command::MergeDriver { base, ours, theirs } => super::merge_driver::run(base, ours, theirs),
        Command::Git { action } => super::git_setup::run(action, cli.dir.as_deref()),
        _ => return None,
    })
}
