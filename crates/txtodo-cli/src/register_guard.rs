//! A command that only reads never registers a folder (task cli-no-implicit-register), except a git
//! checkout's root that holds a list: a repo is never offered to a peer and sidecar identity stamps
//! nothing, so the first read there registers it like the first write (ADR 0043). The global
//! daemon opens and registers any folder a request names, and on register it walked the lists and
//! could stamp `id:` into every line; so `txtodo listall` in an old checkout once rewrote ~120
//! files. Now a read-only command run where no registered workspace is checks first, and fails
//! with the fix instead. A command that writes (`add`, `do`, …) still registers, as designed
//! (task cli-workspace-autoregister). Never a silent fallback to direct-file mode.
//!
//! The same registry answer scopes a `--dir` that points inside a registered workspace (a ref
//! dir, say) to that workspace's list there, as `sub` does, instead of the daemon refusing a
//! folder inside one it already holds (task cli-sub-by-slug).

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

/// `dir` is a git checkout's root that holds a list and no `.txtodo/` yet (`.git` is a folder in a
/// clone and a file in a linked worktree, and `Path::exists` is true for both). Such a folder
/// registers on its first use, read or write. A `.txtodo/` an older build left may say Tagged, and
/// the daemon keeps a stored identity mode: registering it on a read stamped `id:` into every line
/// of this repo's backlog (2026-10-26), so that clone is refused like any unregistered folder.
/// Ref: <https://doc.rust-lang.org/std/path/struct.Path.html#method.exists>
pub fn is_repo_root(dir: &Path) -> bool {
    dir.join(".git").exists()
        && !dir.join(".txtodo").exists()
        && txtodo_workspace_paths::is_workspace_dir(dir)
}

/// For `doctor`, which is no read-only command but must not register either: when `paths.dir` is
/// in no registered workspace, drops the daemon's selector (naming the folder would register it)
/// and returns the warning to show. A per-folder daemon, the default workspace, or a daemon
/// without `WorkspaceList` is left as it is.
pub fn unselect_unregistered(daemon: &mut Daemon, paths: &crate::config::Paths) -> Option<String> {
    if daemon.selector.is_none() || paths.default_workspace {
        return None;
    }
    let roots = daemon.workspace_list().ok()?;
    let here = std::fs::canonicalize(&paths.dir).unwrap_or_else(|_| paths.dir.clone());
    if roots.iter().any(|w| here.starts_with(Path::new(&w.root))) {
        return None;
    }
    daemon.selector = None;
    Some(format!(
        "{} is not a registered workspace; `txtodo workspace add` registers it",
        paths.dir.display()
    ))
}

/// `dir`, or one of its ancestors, is a registered root.
pub fn is_inside(dir: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| dir.starts_with(root))
}

/// A `--dir` inside a registered workspace: its root, and the list there, workspace-relative.
pub struct Scoped {
    pub root: PathBuf,
    pub list: String,
}

/// The list `here` names inside `root`, when it is strictly inside it.
pub fn scoped_list(here: &Path, root: &Path) -> Option<String> {
    let rel = here.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    let parts: Vec<String> = rel
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    Some(format!("{}/todo.txt", parts.join("/")))
}

/// Against the global daemon (`selector` set): a `dir` strictly inside a registered workspace is
/// `Scoped` to it (unless a `--sublist` already scopes the run); a read-only `command` in a folder no
/// registered workspace holds, and that is no git root with a list, is refused with the fix. A per-folder daemon serves its one
/// workspace; an older daemon without `WorkspaceList` is let through, as before.
pub fn check(
    daemon: &mut Daemon,
    dir: &Path,
    command: &Command,
    has_list: bool,
) -> Result<Option<Scoped>, CliError> {
    if daemon.selector.is_none() {
        return Ok(None);
    }
    let Ok(workspaces) = daemon.workspace_list() else {
        return Ok(None);
    };
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let roots: Vec<PathBuf> = workspaces
        .iter()
        .map(|w| canon(Path::new(&w.root)))
        .collect();
    let here = canon(dir);
    if !has_list {
        let inside = roots
            .iter()
            .find_map(|root| scoped_list(&here, root).map(|list| (root.clone(), list)));
        if let Some((root, list)) = inside {
            return Ok(Some(Scoped { root, list }));
        }
    }
    if !is_read_only(command) || is_inside(&here, &roots) || is_repo_root(&here) {
        return Ok(None);
    }
    Err(CliError::Message(format!(
        "{} is not a txtodo workspace, and reading it would register it. \
         Register it with `txtodo workspace add {}`, or read the files as they are with `--no-daemon`",
        here.display(),
        here.display()
    )))
}

/// Runs this command again as `--dir <root> --sublist <list>`, the rest of the arguments as given
/// but the original `--dir`. The child prints its own output and errors.
pub fn rerun_scoped(scoped: &Scoped) -> Result<(), CliError> {
    let mut rest = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "--dir" {
            args.next();
        } else if !a.to_string_lossy().starts_with("--dir=") {
            rest.push(a);
        }
    }
    let exe = std::env::current_exe().map_err(CliError::Io)?;
    let status = std::process::Command::new(exe)
        .arg("--dir")
        .arg(&scoped.root)
        .arg("--sublist")
        .arg(&scoped.list)
        .args(rest)
        .status()
        .map_err(CliError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Reported)
    }
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
    fn a_folder_inside_a_root_is_that_roots_list() {
        let root = Path::new("/w/repo");
        assert_eq!(
            scoped_list(Path::new("/w/repo/tasks/a"), root).as_deref(),
            Some("tasks/a/todo.txt")
        );
        assert_eq!(
            scoped_list(root, root),
            None,
            "the root itself is not scoped"
        );
        assert_eq!(scoped_list(Path::new("/w/other"), root), None);
    }

    #[test]
    fn only_a_git_root_that_holds_a_list_registers_on_a_read() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = |name: &str, git: bool, list: bool| {
            let dir = tmp.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            if git {
                std::fs::create_dir(dir.join(".git")).unwrap();
            }
            if list {
                std::fs::write(dir.join("todo.txt"), "").unwrap();
            }
            dir
        };
        assert!(is_repo_root(&folder("repo", true, true)));
        assert!(!is_repo_root(&folder("plain", false, true)), "no git");
        assert!(!is_repo_root(&folder("bare", true, false)), "no list");
        let old = folder("old", true, true);
        std::fs::create_dir(old.join(".txtodo")).unwrap();
        assert!(
            !is_repo_root(&old),
            "an older build's .txtodo/ may be Tagged"
        );
        let worktree = folder("worktree", false, true);
        std::fs::write(worktree.join(".git"), "gitdir: elsewhere\n").unwrap();
        assert!(
            is_repo_root(&worktree),
            "a linked worktree's .git is a file"
        );
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
