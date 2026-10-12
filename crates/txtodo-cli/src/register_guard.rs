//! Read-only commands never register a folder (tasks/cli-no-implicit-register). The daemon opens
//! and registers any root a request names by path (`WorkspaceSelector.path`), so a `listall` in an
//! old checkout made it a workspace, and the daemon then stamped `id:` into every list there. A
//! read checks the registry first (`WorkspaceList`, which names no workspace and so registers
//! nothing) and, in a folder no workspace covers, fails with the fix. `add` and the other writes
//! still register a fresh folder, as designed (task cli-workspace-autoregister).

use std::path::Path;

use crate::CliError;
use crate::cli::Command;
use crate::client::Daemon;

/// What `main` does with a daemon connection after [`check`].
#[derive(Debug, PartialEq, Eq)]
pub enum Guard {
    /// Talk to the daemon as usual.
    Daemon,
    /// Answer without the daemon: `env` in an unregistered folder.
    Direct,
}

/// `sub` subcommands that only read (todo.sh names and aliases).
const READING_SUBCOMMANDS: &[&str] = &[
    "list", "ls", "listall", "lsa", "listpri", "lsp", "listproj", "lsprj", "listcon", "lsc",
    "listfile", "lf", "log", "blame", "checkout", "lint", "env", "open",
];

/// Whether `command` only reads, so it must not register the folder it runs in.
pub fn reads_only(command: &Command) -> bool {
    match command {
        Command::Sub { cmd, .. } => cmd
            .first()
            .is_some_and(|name| READING_SUBCOMMANDS.contains(&name.as_str())),
        Command::Conflicts { action, .. } => matches!(
            action,
            None | Some(crate::commands::conflicts::Action::List { .. })
        ),
        _ => matches!(
            command,
            Command::Env
                | Command::Lint
                | Command::List { .. }
                | Command::Listall { .. }
                | Command::Listpri { .. }
                | Command::Listproj { .. }
                | Command::Listcon { .. }
                | Command::Listfile { .. }
                | Command::Log { .. }
                | Command::Blame { .. }
                | Command::Checkout { .. }
                | Command::Open { .. }
        ),
    }
}

/// Whether `dir` is a registered workspace's root or inside one. A legacy `--dir`-bridge daemon
/// (no selector) has no registry to ask: it only ever serves its own folder.
pub fn registered(daemon: &mut Daemon, dir: &Path) -> Result<bool, CliError> {
    if daemon.selector.is_none() {
        return Ok(true);
    }
    let Ok(dir) = std::fs::canonicalize(dir) else {
        return Ok(false);
    };
    let roots = daemon.workspace_list()?;
    Ok(roots.iter().any(|w| dir.starts_with(Path::new(&w.root))))
}

/// The guard `main` runs on a daemon connection before anything names the folder by path.
pub fn check(
    command: &Command,
    paths: &crate::config::Paths,
    daemon: &mut Daemon,
) -> Result<Guard, CliError> {
    if paths.default_workspace || !reads_only(command) || registered(daemon, &paths.dir)? {
        return Ok(Guard::Daemon);
    }
    if matches!(command, Command::Env) {
        return Ok(Guard::Direct);
    }
    Err(CliError::Message(format!(
        "{} is not a registered workspace, and reading it through the daemon would register \
         it. `txtodo workspace add` registers it; `--no-daemon` reads the files directly.",
        paths.dir.display()
    )))
}
