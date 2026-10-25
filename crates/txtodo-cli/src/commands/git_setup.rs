//! `txtodo git setup` (task `two-kinds-of-workspace`, ADR 0043): makes a git clone merge its todo
//! lists line by line instead of stopping on a conflict. It does three things, each only if missing,
//! so running it again changes nothing:
//!
//! - `.gitattributes` (committed, so every clone gets it): `todo.txt merge=txtodo`, the root list's
//!   name too when `txtodo.toml` gives it another, and `done.txt merge=union` (it only ever grows,
//!   so git's own union is right there, unlike for `todo.txt` where completing a line moves it);
//! - `merge.txtodo.driver` in this clone's git config (per clone, git never shares it): the command
//!   `txtodo merge-driver %O %A %B`, written with `txtodo` as found on `PATH` and not its resolved
//!   path, since a Homebrew symlink resolves into a versioned Cellar folder an upgrade removes;
//! - `.txtodo/` in `.git/info/exclude`: the daemon's op log would otherwise sit untracked beside the
//!   list, one `git add -A` from being committed. Local to the clone, changes nothing tracked.
//!
//! A clone that never ran this gets git's own merge: it can conflict, it cannot lose a line.
//! Ref: <https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver>,
//! <https://git-scm.com/docs/git-rev-parse#Documentation/git-rev-parse.txt---git-pathltpathgt>

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Subcommand;

use crate::CliError;

/// The `txtodo git` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Makes this clone merge todo lists line by line: `.gitattributes` (commit it), this clone's
    /// `merge.txtodo.driver`, and `.txtodo/` kept out of git. Safe to run again.
    Setup {
        /// Say what would change, and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

const HEADER: &str = "# txtodo: merge todo lists line by line (txtodo git setup)";

/// What a clone still needs. Empty means it is set up.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Lines to add to `.gitattributes`.
    pub attributes: Vec<String>,
    /// `.txtodo/` is missing from `.git/info/exclude`.
    pub exclude: bool,
    /// The driver command to set, when it is not the one set now.
    pub driver: Option<String>,
}

impl Plan {
    /// Nothing to do.
    pub fn is_empty(&self) -> bool {
        *self == Plan::default()
    }
}

/// The attribute lines the lists need: the root list (and `todo.txt`), and `done.txt`.
pub fn wanted_attributes(todo_file: &str) -> Vec<String> {
    let mut lines = vec!["todo.txt merge=txtodo".to_owned()];
    if todo_file != "todo.txt" {
        lines.push(format!("{todo_file} merge=txtodo"));
    }
    lines.push("done.txt merge=union".to_owned());
    lines
}

/// What is missing, given the three files' state: `.gitattributes` and `.git/info/exclude` as text,
/// the driver config now (if any), the attribute lines wanted and the driver command wanted.
pub fn plan(
    attributes: &str,
    exclude: &str,
    driver_now: Option<&str>,
    wanted: &[String],
    driver: &str,
) -> Plan {
    let has = |text: &str, line: &str| text.lines().any(|l| l.trim() == line);
    Plan {
        attributes: wanted
            .iter()
            .filter(|w| !has(attributes, w))
            .cloned()
            .collect(),
        exclude: !(has(exclude, ".txtodo/") || has(exclude, ".txtodo")),
        driver: (driver_now != Some(driver)).then(|| driver.to_owned()),
    }
}

/// The driver command git runs: `<txtodo> merge-driver %O %A %B`, the path quoted for the shell git
/// runs it through when it needs it.
pub fn driver_command(txtodo: &Path) -> String {
    let path = txtodo.to_string_lossy();
    let plain = path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c));
    let path = if plain {
        path.into_owned()
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    };
    format!("{path} merge-driver %O %A %B")
}

/// `txtodo` as found on `path_var`, when that is this very binary; else `current`. Not canonical:
/// the entry on `PATH` is the stable name, the resolved file may be gone after an upgrade.
pub fn txtodo_path(path_var: Option<&OsStr>, current: &Path) -> PathBuf {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let this = canon(current);
    let found = path_var
        .into_iter()
        .flat_map(std::env::split_paths)
        .find_map(|dir| {
            let candidate = dir.join("txtodo");
            (candidate.is_file() && canon(&candidate) == this).then_some(candidate)
        });
    found.unwrap_or_else(|| current.to_path_buf())
}

/// Runs a subcommand. `dir` is `--dir`, else the current folder: the clone is the nearest folder at
/// or above it holding `.git`.
pub fn run(action: &Action, dir: Option<&str>) -> Result<(), CliError> {
    let Action::Setup { dry_run } = action;
    let start = match dir {
        Some(d) => PathBuf::from(d),
        None => std::env::current_dir().map_err(CliError::Io)?,
    };
    let root = repo_root(&start).ok_or_else(|| {
        CliError::Message(format!(
            "txtodo: {} is not inside a git clone (no .git above it)",
            start.display()
        ))
    })?;
    let todo_file = crate::config_root_list::root_list_name(&root).0;
    let exclude_path = git_path(&root, "info/exclude")?;
    let current = std::env::current_exe().map_err(CliError::Io)?;
    let driver = driver_command(&txtodo_path(std::env::var_os("PATH").as_deref(), &current));
    let plan = plan(
        &read_or_empty(&root.join(".gitattributes")),
        &read_or_empty(&exclude_path),
        git_config_get(&root, "merge.txtodo.driver").as_deref(),
        &wanted_attributes(&todo_file),
        &driver,
    );
    if plan.is_empty() {
        println!(
            "TODO: {} is already set up for txtodo lists",
            root.display()
        );
        return Ok(());
    }
    if !dry_run {
        apply(&root, &exclude_path, &plan)?;
    }
    report(&plan, *dry_run);
    Ok(())
}

fn report(plan: &Plan, dry_run: bool) {
    let (add, set) = if dry_run {
        ("would add", "would set")
    } else {
        ("added", "set")
    };
    for line in &plan.attributes {
        println!("TODO: {add} to .gitattributes: {line}");
    }
    if let Some(driver) = &plan.driver {
        println!("TODO: {set} merge.txtodo.driver = {driver}");
    }
    if plan.exclude {
        println!("TODO: {add} .txtodo/ to .git/info/exclude");
    }
    if !plan.attributes.is_empty() && !dry_run {
        println!(
            "TODO: commit .gitattributes so your other clones get it, then run txtodo git setup in each"
        );
    }
}

fn apply(root: &Path, exclude_path: &Path, plan: &Plan) -> Result<(), CliError> {
    if !plan.attributes.is_empty() {
        let path = root.join(".gitattributes");
        let mut text = read_or_empty(&path);
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        if !text.contains(HEADER) {
            text.push_str(HEADER);
            text.push('\n');
        }
        for line in &plan.attributes {
            text.push_str(line);
            text.push('\n');
        }
        write(&path, &text)?;
    }
    if plan.exclude {
        let mut text = read_or_empty(exclude_path);
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(".txtodo/\n");
        if let Some(parent) = exclude_path.parent() {
            std::fs::create_dir_all(parent).map_err(CliError::Io)?;
        }
        write(exclude_path, &text)?;
    }
    if let Some(driver) = &plan.driver {
        git(root, &["config", "--local", "merge.txtodo.driver", driver])?;
        git(
            root,
            &[
                "config",
                "--local",
                "merge.txtodo.name",
                "txtodo todo lists",
            ],
        )?;
    }
    Ok(())
}

/// The nearest folder at or above `start` that holds `.git` (a folder in a clone, a file in a
/// linked worktree).
fn repo_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

fn read_or_empty(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn write(path: &Path, text: &str) -> Result<(), CliError> {
    std::fs::write(path, text)
        .map_err(|e| CliError::Message(format!("txtodo: cannot write {}: {e}", path.display())))
}

/// `git -C root <args>`; its trimmed stdout, or an error naming what failed.
fn git(root: &Path, args: &[&str]) -> Result<String, CliError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        // Git exports these to a hook it runs; they would point this at that clone, not `root`.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .map_err(|e| CliError::Message(format!("txtodo: git setup needs git on PATH ({e})")))?;
    if !out.status.success() {
        return Err(CliError::Message(format!(
            "txtodo: git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The value of a git config key in this clone, or `None` when unset.
fn git_config_get(root: &Path, key: &str) -> Option<String> {
    git(root, &["config", "--local", "--get", key])
        .ok()
        .filter(|v| !v.is_empty())
}

/// Where git keeps `rel` for this clone (a linked worktree shares its main clone's).
fn git_path(root: &Path, rel: &str) -> Result<PathBuf, CliError> {
    let printed = git(root, &["rev-parse", "--git-path", rel])?;
    let path = PathBuf::from(printed);
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

#[cfg(test)]
#[path = "git_setup_tests.rs"]
mod tests;
