//! `txtodo repos` (task `two-kinds-of-workspace`, ADR 0043): the git clones with a todo list that
//! the daemon knows. git syncs a repo's list; txtodo does not. What txtodo keeps is the daemon
//! watching the list (a `git pull` is an outside edit it takes in) and the registry row that lets a
//! client find it by name. The daemon looks for clones under its scan roots (the home folder by
//! default) at start and every six hours; `scan` looks now, `add DIR` takes one it would not find,
//! and `roots` shows or changes where it looks.
//!
//! Device-level, like `workspace`: none names the folder you stand in, so none registers it.

use std::time::{SystemTime, UNIX_EPOCH};

use clap::Subcommand;
use txtodo_proto::v1 as pb;

use crate::client::Daemon;
use crate::{CliError, json};

/// The `txtodo repos` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Every repo workspace, and when the daemon last looked for more (the default).
    #[command(visible_alias = "list")]
    Ls,
    /// Looks for repos under the scan roots now. Waits for it unless `--background`.
    Scan {
        /// Start the scan and return at once.
        #[arg(long)]
        background: bool,
    },
    /// Registers one repo the scan would not find (outside the roots, or a worktree or submodule).
    Add {
        /// The folder; it is registered where it is, never copied.
        dir: String,
    },
    /// The folders the daemon scans for repos (the default), or `add DIR` / `rm DIR`.
    Roots {
        #[command(subcommand)]
        action: Option<RootsAction>,
    },
}

/// What `txtodo repos roots` changes.
#[derive(Debug, Subcommand)]
pub enum RootsAction {
    /// Scan under DIR too.
    Add {
        /// An absolute folder.
        dir: String,
    },
    /// Stop scanning under DIR. Removing every root means the daemon scans nothing.
    #[command(visible_alias = "remove")]
    Rm {
        /// A folder that is a scan root now.
        dir: String,
    },
}

/// Runs a `repos` subcommand against the global daemon.
pub fn run(daemon: &mut Daemon, action: Option<&Action>, as_json: bool) -> Result<(), CliError> {
    match action {
        None | Some(Action::Ls) => run_ls(daemon, as_json),
        Some(Action::Scan { background }) => run_scan(daemon, *background, as_json),
        Some(Action::Add { dir }) => run_add(daemon, dir, as_json),
        Some(Action::Roots { action }) => run_roots(daemon, action.as_ref(), as_json),
    }
}

/// The state, without changing anything.
fn state(daemon: &mut Daemon) -> Result<pb::ReposResponse, CliError> {
    Ok(daemon.repos(pb::ReposRequest::default())?)
}

fn run_ls(daemon: &mut Daemon, as_json: bool) -> Result<(), CliError> {
    let repos: Vec<pb::WorkspaceInfo> = daemon
        .workspace_list()?
        .into_iter()
        .filter(|w| w.kind() == pb::WorkspaceKind::Repo && !w.is_remote)
        .collect();
    let scan = state(daemon)?;
    for w in &repos {
        if as_json {
            println!(
                r#"{{"id":{},"name":{},"root":{}}}"#,
                json::str(&w.workspace_id),
                json::str(&w.name),
                json::str(&w.root)
            );
        } else {
            println!("{}  {}  {}", w.name, w.workspace_id, w.root);
        }
    }
    if !as_json {
        println!(
            "TODO: {} repo(s); {}",
            repos.len(),
            summary(&scan, now_ms())
        );
        if let Some(note) = skipped_note(&scan.roots) {
            println!("TODO: {note}");
        }
    }
    Ok(())
}

fn run_scan(daemon: &mut Daemon, background: bool, as_json: bool) -> Result<(), CliError> {
    let req = pb::ReposRequest {
        scan: true,
        wait: !background,
        ..pb::ReposRequest::default()
    };
    let got = daemon.repos(req)?;
    if as_json {
        println!("{}", scan_json(&got));
    } else if background {
        println!("TODO: scan started; `txtodo repos` shows when it is done");
    } else {
        println!(
            "TODO: found {} repo(s), registered {} new",
            got.found, got.registered
        );
    }
    Ok(())
}

fn run_add(daemon: &mut Daemon, dir: &str, as_json: bool) -> Result<(), CliError> {
    let root = std::fs::canonicalize(dir)
        .map_err(|e| CliError::Message(format!("txtodo: cannot read {dir}: {e}")))?;
    if !root.join(".git").exists() {
        return Err(CliError::Message(format!(
            "txtodo: {} is not a git clone (no .git); register any other folder with `txtodo workspace add`",
            root.display()
        )));
    }
    let info = daemon.workspace_add(&root.display().to_string())?;
    if as_json {
        println!(
            r#"{{"id":{},"root":{}}}"#,
            json::str(&info.workspace_id),
            json::str(&info.root)
        );
    } else {
        println!("TODO: registered {} ({})", info.root, info.workspace_id);
    }
    Ok(())
}

fn run_roots(
    daemon: &mut Daemon,
    action: Option<&RootsAction>,
    as_json: bool,
) -> Result<(), CliError> {
    let mut req = pb::ReposRequest::default();
    match action {
        Some(RootsAction::Add { dir }) => req.add_roots.push(absolute(dir)?),
        Some(RootsAction::Rm { dir }) => req.remove_roots.push(absolute(dir)?),
        None => {}
    }
    let got = daemon.repos(req)?;
    for root in &got.roots {
        if as_json {
            println!(r#"{{"root":{}}}"#, json::str(root));
        } else {
            println!("{root}");
        }
    }
    if got.roots.is_empty() && !as_json {
        println!("TODO: no scan roots; the daemon looks for no repos");
    }
    if let (false, Some(note)) = (as_json, skipped_note(&got.roots)) {
        println!("TODO: {note}");
    }
    Ok(())
}

/// `dir` as an absolute path, resolving `~`-less relative names against the current folder. The
/// folder need not exist: a root may be removed after the folder is.
fn absolute(dir: &str) -> Result<String, CliError> {
    let path = std::path::Path::new(dir);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(CliError::Io)?.join(path)
    };
    Ok(full.display().to_string())
}

fn scan_json(got: &pb::ReposResponse) -> String {
    format!(
        r#"{{"found":{},"registered":{},"running":{},"last_scan_ms":{}}}"#,
        got.found, got.registered, got.running, got.last_scan_ms
    )
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// `scanning N root(s), last scan 3 h ago`, for `repos` and `doctor`.
pub fn summary(scan: &pb::ReposResponse, now_ms: u64) -> String {
    let roots = match scan.roots.len() {
        0 => "no scan roots".to_owned(),
        n => format!("scanning {n} root(s): {}", scan.roots.join(", ")),
    };
    let last = if scan.running {
        "a scan is running".to_owned()
    } else if scan.last_scan_ms == 0 {
        "never scanned yet".to_owned()
    } else {
        format!(
            "last scan {}",
            ago(now_ms.saturating_sub(scan.last_scan_ms))
        )
    };
    format!("{roots}; {last}")
}

/// What a scan leaves alone on macOS, for `repos`, `repos roots` and `doctor`: the home folder's
/// gated folders that a root walks past (the home folder is one) and none links. Reading one raises
/// a system permission prompt, so the daemon skips them until a root is at or inside one. `None`
/// when nothing is left alone (not macOS, no root above them, or all linked).
pub fn skipped_note(roots: &[String]) -> Option<String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let home = home.canonicalize().unwrap_or(home);
    note_for(
        &txtodo_workspace_paths::protected_home_folders(&home),
        roots,
    )
}

fn note_for(gated: &[std::path::PathBuf], roots: &[String]) -> Option<String> {
    let roots: Vec<&std::path::Path> = roots.iter().map(std::path::Path::new).collect();
    let skipped: Vec<String> = gated
        .iter()
        .filter(|g| roots.iter().any(|r| g.starts_with(r) && *g != r))
        .filter(|g| !roots.iter().any(|r| r.starts_with(g)))
        .filter_map(|g| g.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    if skipped.is_empty() {
        return None;
    }
    Some(format!(
        "the scan leaves {} alone: macOS asks before a program reads them. \
         `txtodo repos roots add ~/{}` scans one (macOS then asks once)",
        skipped.join(", "),
        skipped[0]
    ))
}

/// `just now`, `5 min ago`, `3 h ago`, `2 d ago`.
fn ago(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        0..=59 => "just now".to_owned(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(roots: &[&str], last_scan_ms: u64, running: bool) -> pb::ReposResponse {
        pb::ReposResponse {
            roots: roots.iter().map(|r| (*r).to_owned()).collect(),
            last_scan_ms,
            running,
            ..pb::ReposResponse::default()
        }
    }

    #[test]
    fn the_summary_says_where_it_looks_and_when_it_last_did() {
        let hour = 3_600_000;
        let s = summary(&scan(&["/home/a"], 1_000_000, false), 1_000_000 + 3 * hour);
        assert_eq!(s, "scanning 1 root(s): /home/a; last scan 3 h ago");
        assert!(summary(&scan(&[], 0, false), 5).contains("no scan roots; never scanned yet"));
        assert!(summary(&scan(&["/x", "/y"], 0, true), 5).contains("a scan is running"));
    }

    #[test]
    fn the_note_names_the_gated_folders_a_root_walks_past_and_none_links() {
        use std::path::PathBuf;
        let gated: Vec<PathBuf> = ["Desktop", "Documents", "Downloads"]
            .iter()
            .map(|n| PathBuf::from("/Users/a").join(n))
            .collect();
        let home = vec!["/Users/a".to_owned()];
        let note = note_for(&gated, &home).unwrap();
        assert!(
            note.contains("leaves Desktop, Documents, Downloads alone"),
            "{note}"
        );
        assert!(
            note.contains("macOS asks") && note.contains("roots add ~/Desktop"),
            "{note}"
        );

        // One linked as a root, or a root inside it: no longer left alone.
        let linked = vec!["/Users/a".to_owned(), "/Users/a/Documents".to_owned()];
        let note = note_for(&gated, &linked).unwrap();
        assert!(note.contains("leaves Desktop, Downloads alone"), "{note}");
        let inside = vec!["/Users/a".to_owned(), "/Users/a/Documents/code".to_owned()];
        let note = note_for(&gated, &inside).unwrap();
        assert!(note.contains("leaves Desktop, Downloads alone"), "{note}");

        // A root that walks past none of them, no roots at all, or nothing gated: nothing to say.
        assert!(note_for(&gated, &["/Users/a/Development".to_owned()]).is_none());
        assert!(note_for(&gated, &[]).is_none());
        assert!(note_for(&[], &home).is_none());
    }

    #[test]
    fn ages_read_in_the_largest_whole_unit() {
        assert_eq!(ago(30_000), "just now");
        assert_eq!(ago(5 * 60_000), "5 min ago");
        assert_eq!(ago(2 * 86_400_000), "2 d ago");
    }

    #[test]
    fn a_relative_root_is_made_absolute_and_an_absolute_one_kept() {
        assert_eq!(absolute("/home/a/code").unwrap(), "/home/a/code");
        let got = absolute("code").unwrap();
        assert!(
            std::path::Path::new(&got).is_absolute() && got.ends_with("code"),
            "{got}"
        );
    }

    #[test]
    fn the_scan_json_carries_the_counts() {
        let got = pb::ReposResponse {
            found: 12,
            registered: 3,
            last_scan_ms: 7,
            ..pb::ReposResponse::default()
        };
        assert_eq!(
            scan_json(&got),
            r#"{"found":12,"registered":3,"running":false,"last_scan_ms":7}"#
        );
    }
}
