//! `txtodo lists` and `--list NAME` (task `two-kinds-of-workspace`, ADR 0043): named lists that
//! txtodo syncs between the user's own devices, as the default workspace does. A list is a
//! workspace under the daemon's `lists/` folder whose id comes from its name, so the same name on
//! two devices is one list. `new` makes one, `ls` shows them, `rename` changes what one shows (its
//! id and folder stay), `rm` takes one off this device (its folder moves aside, never deleted, and
//! the other devices keep it), `import` copies a folder into a new one (`lists_import.rs`).
//!
//! Device-level, like `workspace`: none of these names a folder, so none registers the cwd. They
//! need the global daemon (`needs_daemon`); a legacy `--dir` bridge has no lists to answer with.

use std::path::PathBuf;

use clap::Subcommand;
use txtodo_proto::v1 as pb;
use txtodo_workspace_paths::{ListRef, Pick, pick_list};

use crate::client::Daemon;
use crate::config::Env;
use crate::{CliError, json};

/// The `txtodo lists` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Makes a list. Its name is 1 to 48 characters from a-z, 0-9, `-` and `_` (case is folded),
    /// and it is the same list on every one of your devices that makes or receives it.
    New {
        /// The list's name.
        name: String,
    },
    /// Every list on this device, the default included (the default).
    #[command(visible_alias = "list")]
    Ls,
    /// Changes the name a list shows. Its id and folder stay, so the name it was made under still
    /// finds it. Every one of your devices shows the new name.
    Rename {
        /// The list: the name it shows, or the name it was made under.
        list: String,
        /// The name to show.
        new_name: String,
    },
    /// Takes a list off this device: its folder moves into a time-stamped sibling (printed),
    /// never deleted. Your other devices keep the list. The default cannot be removed.
    #[command(visible_alias = "remove")]
    Rm {
        /// The list: the name it shows, or the name it was made under.
        list: String,
    },
    /// Copies a folder's `todo.txt`, `tasks/` and the rest of its files (hidden files and `.txtodo/`
    /// left out) into a new list, so a folder that used to sync can keep syncing. The list starts
    /// with a fresh history.
    Import {
        /// The folder to copy from; it must hold a `todo.txt`.
        dir: String,
        /// The new list's name; defaults to the folder's name.
        name: Option<String>,
    },
}

/// Runs a `lists` subcommand against the global daemon; no subcommand is `ls`.
pub fn run(daemon: &mut Daemon, action: Option<&Action>, as_json: bool) -> Result<(), CliError> {
    match action {
        None | Some(Action::Ls) => run_ls(daemon, as_json),
        Some(Action::New { name }) => run_new(daemon, name, as_json),
        Some(Action::Rename { list, new_name }) => run_rename(daemon, list, new_name, as_json),
        Some(Action::Rm { list }) => run_rm(daemon, list, as_json),
        Some(Action::Import { dir, name }) => {
            super::lists_import::run(daemon, dir, name.as_deref(), as_json)
        }
    }
}

/// `--list NAME`: the folder of the list `name` means, asked of the global daemon (started first
/// when it is down). The caller treats it as `--dir <folder>`.
pub fn dir_of(env: &Env, name: &str, no_daemon: bool) -> Result<PathBuf, CliError> {
    if no_daemon {
        return Err(CliError::Message(
            "txtodo: --list needs the daemon; drop --no-daemon, or give the list's folder with --dir"
                .to_owned(),
        ));
    }
    let mut daemon = connect(env)?;
    let workspaces = daemon.workspace_list()?;
    root_of(&workspaces, name).map(PathBuf::from)
}

/// The global daemon, started if it is not running.
fn connect(env: &Env) -> Result<Daemon, CliError> {
    if let Some(daemon) = Daemon::global(env)? {
        return Ok(daemon);
    }
    crate::daemon_ensure::start_global(env);
    Daemon::global(env)?.ok_or_else(|| {
        CliError::Message(format!(
            "txtodo: {}",
            crate::commands::history::NEEDS_DAEMON
        ))
    })
}

/// The lists among `workspaces`: the default and anything the daemon calls a list.
fn lists_of(workspaces: &[pb::WorkspaceInfo]) -> Vec<&pb::WorkspaceInfo> {
    let is_list = |w: &&pb::WorkspaceInfo| w.kind() == pb::WorkspaceKind::List;
    workspaces.iter().filter(is_list).collect()
}

/// The root of the list `name` means among `workspaces`, by the rule every client shares
/// (`txtodo_workspace_paths::pick_list`).
fn root_of<'a>(workspaces: &'a [pb::WorkspaceInfo], name: &str) -> Result<&'a str, CliError> {
    let lists = lists_of(workspaces);
    if lists.is_empty() {
        return Err(CliError::Message(
            "txtodo: this daemon does not know named lists; run `txtodo daemon install --force`"
                .to_owned(),
        ));
    }
    let roots: Vec<PathBuf> = lists.iter().map(|w| PathBuf::from(&w.root)).collect();
    let refs: Vec<ListRef<'_>> = lists
        .iter()
        .zip(&roots)
        .map(|(w, root)| ListRef {
            shown: &w.name,
            root,
        })
        .collect();
    match pick_list(&refs, name) {
        Pick::Found(i) => Ok(&lists[i].root),
        Pick::Ambiguous(_) => Err(CliError::Message(format!(
            "txtodo: more than one list shows {name:?}; name one by the name it was made under \
             (the last part of its folder in `txtodo lists`)"
        ))),
        Pick::NotFound => Err(CliError::Message(format!(
            "txtodo: no list named {name:?}; make it with `txtodo lists new {name}`, or see them \
             with `txtodo lists`"
        ))),
    }
}

fn row_text(w: &pb::WorkspaceInfo) -> String {
    let default = if w.is_default { " [default]" } else { "" };
    format!("{}  {}  {}{default}", w.name, w.workspace_id, w.root)
}

fn row_json(w: &pb::WorkspaceInfo) -> String {
    format!(
        r#"{{"name":{},"id":{},"root":{},"is_default":{}}}"#,
        json::str(&w.name),
        json::str(&w.workspace_id),
        json::str(&w.root),
        w.is_default
    )
}

fn print_row(w: &pb::WorkspaceInfo, as_json: bool) {
    println!("{}", if as_json { row_json(w) } else { row_text(w) });
}

fn run_ls(daemon: &mut Daemon, as_json: bool) -> Result<(), CliError> {
    let workspaces = daemon.workspace_list()?;
    let lists = lists_of(&workspaces);
    if lists.is_empty() && !workspaces.is_empty() {
        // Every row has no kind: a daemon from before named lists.
        root_of(&workspaces, "default")?;
    }
    for w in lists {
        print_row(w, as_json);
    }
    Ok(())
}

fn run_new(daemon: &mut Daemon, name: &str, as_json: bool) -> Result<(), CliError> {
    let made = daemon.list_ensure(name)?;
    let Some(info) = made.workspace else {
        return Err(CliError::Message(
            "txtodo: the daemon made no list".to_owned(),
        ));
    };
    if as_json {
        print_row(&info, true);
    } else if made.created {
        println!("TODO: made list {} at {}", info.name, info.root);
    } else {
        println!("TODO: list {} is already here at {}", info.name, info.root);
    }
    Ok(())
}

fn run_rename(
    daemon: &mut Daemon,
    list: &str,
    new_name: &str,
    as_json: bool,
) -> Result<(), CliError> {
    let info = daemon.list_rename(list, new_name)?;
    if as_json {
        print_row(&info, true);
    } else {
        println!("TODO: list {list} now shows as {}", info.name);
    }
    Ok(())
}

fn run_rm(daemon: &mut Daemon, list: &str, as_json: bool) -> Result<(), CliError> {
    let gone = daemon.list_remove(list)?;
    if as_json {
        println!(
            r#"{{"removed":{},"moved_to":{}}}"#,
            gone.removed,
            json::str(&gone.moved_to)
        );
    } else {
        println!(
            "TODO: list {list} is off this device; its folder is now {} (your other devices keep it)",
            gone.moved_to
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(name: &str, id: &str, root: &str, kind: pb::WorkspaceKind) -> pb::WorkspaceInfo {
        pb::WorkspaceInfo {
            workspace_id: id.into(),
            name: name.into(),
            root: root.into(),
            kind: kind as i32,
            ..pb::WorkspaceInfo::default()
        }
    }

    fn two_lists_and_a_repo() -> Vec<pb::WorkspaceInfo> {
        vec![
            list("default", "01D", "/d/default", pb::WorkspaceKind::List),
            list("projects", "01W", "/d/lists/work", pb::WorkspaceKind::List),
            list("txtodo", "01R", "/home/a/txtodo", pb::WorkspaceKind::Repo),
        ]
    }

    #[test]
    fn a_name_finds_a_list_by_what_it_shows_or_was_made_under() {
        let all = two_lists_and_a_repo();
        assert_eq!(root_of(&all, "projects").unwrap(), "/d/lists/work");
        assert_eq!(root_of(&all, "Work").unwrap(), "/d/lists/work");
        assert_eq!(root_of(&all, "default").unwrap(), "/d/default");
    }

    #[test]
    fn a_repo_is_never_a_list() {
        let all = two_lists_and_a_repo();
        let why = root_of(&all, "txtodo").unwrap_err().to_string();
        assert!(why.contains("lists new txtodo"), "{why}");
    }

    #[test]
    fn a_daemon_without_lists_says_how_to_upgrade() {
        let old = vec![list("x", "01X", "/x", pb::WorkspaceKind::Unspecified)];
        let why = root_of(&old, "x").unwrap_err().to_string();
        assert!(why.contains("daemon install --force"), "{why}");
    }

    #[test]
    fn two_lists_showing_one_name_are_ambiguous() {
        let all = vec![
            list("x", "01A", "/d/lists/a", pb::WorkspaceKind::List),
            list("x", "01B", "/d/lists/b", pb::WorkspaceKind::List),
        ];
        assert!(
            root_of(&all, "x")
                .unwrap_err()
                .to_string()
                .contains("more than one")
        );
        assert_eq!(root_of(&all, "b").unwrap(), "/d/lists/b");
    }

    #[test]
    fn a_row_prints_as_text_and_as_json() {
        let w = list("work", "01W", "/d/lists/work", pb::WorkspaceKind::List);
        assert_eq!(row_text(&w), "work  01W  /d/lists/work");
        assert_eq!(
            row_json(&w),
            r#"{"name":"work","id":"01W","root":"/d/lists/work","is_default":false}"#
        );
    }
}
