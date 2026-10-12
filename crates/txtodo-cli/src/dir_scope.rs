//! `--dir` naming a folder inside a registered workspace (tasks/cli-sub-by-slug): the command runs
//! on that workspace, scoped to the folder's list with the hidden `--list`, exactly as `sub` does,
//! instead of the daemon refusing to register a folder inside another workspace.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::CliError;
use crate::client::Daemon;

/// The registered workspace root strictly containing `dir`, and `dir`'s list relative to it, or
/// `None` when `dir` is a root itself, outside every workspace, or the daemon has no registry.
pub fn nested(daemon: &mut Daemon, dir: &Path) -> Result<Option<(PathBuf, String)>, CliError> {
    if daemon.selector.is_none() {
        return Ok(None);
    }
    // Only a folder that holds a list: from any other subfolder a write would start a new one.
    let Ok(dir) = std::fs::canonicalize(dir) else {
        return Ok(None);
    };
    if !dir.join("todo.txt").is_file() {
        return Ok(None);
    }
    let root = daemon
        .workspace_list()?
        .into_iter()
        .map(|w| PathBuf::from(w.root))
        .filter(|root| dir.starts_with(root) && dir != *root)
        .max_by_key(|root| root.as_os_str().len());
    let Some(root) = root else {
        return Ok(None);
    };
    let Ok(rel) = dir.strip_prefix(&root) else {
        return Ok(None);
    };
    // Workspace-relative with `/` separators, like every list path the daemon knows.
    let rel: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(Some((root, format!("{}/todo.txt", rel.join("/")))))
}

/// [`rerun`] when `--dir` named a folder inside a registered workspace and nothing scoped the
/// command yet (`--list`, or a `workspace` command, which is about the registry itself).
pub fn scoped(
    command: &crate::cli::Command,
    has_list: bool,
    daemon: &mut Daemon,
    dir: &Path,
) -> Result<Option<Result<(), CliError>>, CliError> {
    if has_list || matches!(command, crate::cli::Command::Workspace { .. }) {
        return Ok(None);
    }
    Ok(nested(daemon, dir)?.map(|(root, list)| rerun(&root, &list)))
}

/// Runs this binary again with `--dir <root> --list <list>` and every other argument as given.
pub fn rerun(root: &Path, list: &str) -> Result<(), CliError> {
    let exe = std::env::current_exe().map_err(CliError::Io)?;
    let status = std::process::Command::new(exe)
        .arg("--dir")
        .arg(root)
        .arg("--list")
        .arg(list)
        .args(without_dir(std::env::args_os().skip(1)))
        .status()
        .map_err(CliError::Io)?;
    if status.success() {
        Ok(())
    } else {
        // The child already printed its own error; only the exit status carries over.
        Err(CliError::Reported)
    }
}

/// `args` without `--dir X` / `--dir=X`.
fn without_dir(args: impl Iterator<Item = OsString>) -> Vec<OsString> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        let text = arg.to_string_lossy();
        if text == "--dir" {
            skip_next = true;
        } else if !text.starts_with("--dir=") {
            out.push(arg);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_dir_forms_are_dropped_and_the_rest_kept_in_order() {
        let args = ["--dir", "tasks/x", "ls", "--dir=y", "+home"].map(OsString::from);
        let kept = without_dir(args.into_iter());
        assert_eq!(kept, ["ls", "+home"].map(OsString::from));
        assert!(without_dir(std::iter::empty()).is_empty());
    }
}
