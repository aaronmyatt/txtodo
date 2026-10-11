//! A command that only reads never registers a folder (task cli-no-implicit-register). The global
//! daemon opens and registers any folder a request names, and on register it walked the lists and
//! could stamp `id:` into every line; so `txtodo listall` in an old checkout once rewrote ~120
//! files. Now a read-only command run where no registered workspace is checks first, and fails
//! with the fix instead. A command that writes (`add`, `do`, …) still registers, as designed
//! (task cli-workspace-autoregister). Never a silent fallback to direct-file mode.

use std::path::{Path, PathBuf};

use crate::CliError;
use crate::cli::Command;
use crate::client::Daemon;

/// The todo.sh names (and aliases) of the read-only commands, as `sub` passes them on.
const READ_ONLY_NAMES: [&str; 19] = [
    "list",
    "ls",
    "listall",
    "lsa",
    "listpri",
    "lsp",
    "listproj",
    "lsprj",
    "listcon",
    "lsc",
    "listfile",
    "lf",
    "lint",
    "env",
    "log",
    "blame",
    "checkout",
    "open",
    "conflicts",
];

/// Whether `command` only reads.
pub fn is_read_only(command: &Command) -> bool {
    use crate::commands::conflicts::Action as Conflicts;
    match command {
        Command::List { .. }
        | Command::Listall { .. }
        | Command::Listpri { .. }
        | Command::Listproj { .. }
        | Command::Listcon { .. }
        | Command::Listfile { .. }
        | Command::Lint
        | Command::Env
        | Command::Log { .. }
        | Command::Blame { .. }
        | Command::Checkout { .. }
        | Command::Open { .. } => true,
        Command::Conflicts { action } => matches!(action, None | Some(Conflicts::List { .. })),
        Command::Prune { yes, .. } => !yes,
        Command::Sub { cmd, .. } => cmd
            .first()
            .is_some_and(|name| READ_ONLY_NAMES.contains(&name.as_str()) && name != "conflicts"),
        _ => false,
    }
}

/// `dir`, or one of its ancestors, is a registered root.
pub fn is_inside(dir: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| dir.starts_with(root))
}

/// Refuses a read-only `command` in `dir` when no registered workspace holds it. Only for the
/// global daemon (`selector` set); a per-folder daemon serves its one workspace. An older daemon
/// without `WorkspaceList` is let through, as before.
pub fn check(daemon: &mut Daemon, dir: &Path, command: &Command) -> Result<(), CliError> {
    if daemon.selector.is_none() || !is_read_only(command) {
        return Ok(());
    }
    let Ok(workspaces) = daemon.workspace_list() else {
        return Ok(());
    };
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let roots: Vec<PathBuf> = workspaces
        .iter()
        .map(|w| canon(Path::new(&w.root)))
        .collect();
    let here = canon(dir);
    if is_inside(&here, &roots) {
        return Ok(());
    }
    Err(CliError::Message(format!(
        "{} is not a txtodo workspace, and reading it would register it. \
         Register it with `txtodo workspace add {}`, or read the files as they are with `--no-daemon`",
        here.display(),
        here.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parsed(args: &[&str]) -> Command {
        let mut all = vec!["txtodo"];
        all.extend_from_slice(args);
        crate::cli::Cli::parse_from(all).command
    }

    #[test]
    fn reads_are_read_only_and_writes_are_not() {
        for args in [
            &["listall"][..],
            &["ls", "milk"],
            &["lint"],
            &["log"],
            &["conflicts"],
            &["sub", "1", "listall"],
            &["prune", "--orphans"],
        ] {
            assert!(is_read_only(&parsed(args)), "{args:?}");
        }
        for args in [
            &["add", "x"][..],
            &["do", "1"],
            &["sub", "1", "do", "2"],
            &["prune", "--orphans", "--yes"],
            &["conflicts", "keep-newest", "--yes"],
        ] {
            assert!(!is_read_only(&parsed(args)), "{args:?}");
        }
    }

    #[test]
    fn a_folder_inside_a_registered_root_counts() {
        let roots = [PathBuf::from("/w/repo")];
        assert!(is_inside(Path::new("/w/repo"), &roots));
        assert!(is_inside(Path::new("/w/repo/tasks/a"), &roots));
        assert!(!is_inside(Path::new("/w/other"), &roots));
        assert!(
            !is_inside(Path::new("/w/repository"), &roots),
            "a prefix of the name is not inside"
        );
    }
}
