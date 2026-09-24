//! `txtodo workspace add|remove|list` (ADR 0025, task `cli-workspace-commands`): manages the
//! device-global daemon's workspace registry. `offers|accept|decline` (task
//! `workspace-offer-cli`) live in `workspace_offers.rs`; only their clap variants are here. Needs the true global daemon — a legacy
//! `--dir`-bridge daemon has no registry to answer these with (`Daemon::workspace_add`/etc.
//! return an `Unimplemented` `ClientError::Rpc` against one, surfaced as a normal error).

use crate::client::Daemon;
use crate::config::Env;
use crate::{CliError, Ctx, json};
use clap::Subcommand;
use txtodo_proto::v1 as pb;

/// The `txtodo workspace` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Registers a directory (default: the resolved `--dir`/cwd) without opening it.
    Add {
        /// The directory to register; defaults to the resolved `--dir`/cwd.
        dir: Option<String>,
    },
    /// Un-registers a workspace id; never touches its `.txtodo/` state on disk.
    #[command(visible_alias = "rm")]
    Remove {
        /// The workspace's id (ULID text, from `workspace list`).
        id: String,
    },
    /// id, root, added time, and whether it still exists / has state on disk.
    #[command(visible_alias = "ls")]
    List,
    /// Shows where this workspace keeps its root list and its `ref:` folders, or changes them
    /// (written to `txtodo.toml`). A change is refused while ref dirs sit in the old place, unless
    /// `--move` moves them.
    Layout {
        /// Folder for the ref dirs of root-list lines, relative to the workspace (`.` = beside the
        /// list). Omitted keeps the current one.
        #[arg(long)]
        refs_dir: Option<String>,
        /// The root list's path, relative to the workspace (e.g. `work.txt`); the daemon creates
        /// it when missing. Omitted keeps the current one.
        #[arg(long)]
        todo_file: Option<String>,
        /// Move the ref dirs to the new place instead of refusing.
        #[arg(long = "move")]
        move_dirs: bool,
    },
    /// Workspaces a paired peer offered this device, not yet accepted or declined: one row per
    /// offer, workspace id first.
    Offers,
    /// Mirrors a pending offer now, into the daemon's own folder (it also does this on its own):
    /// same workspace id as the peer, so the two sync as one workspace. `--from` picks the device
    /// when several peers offer the same id.
    Accept {
        /// The offered workspace's id (ULID text, from `workspace offers`).
        id: String,
        /// The offering device's id, when more than one device offers this workspace.
        #[arg(long)]
        from: Option<String>,
    },
    /// Discards a pending offer.
    Decline {
        /// The offered workspace's id (ULID text, from `workspace offers`).
        id: String,
        /// The offering device's id, when more than one device offers this workspace.
        #[arg(long)]
        from: Option<String>,
    },
    /// Prints the default workspace's directory (the folder Finder will not show), and whether it
    /// exists yet. Needs no daemon.
    Default,
    /// Un-registers every workspace whose root no longer exists on disk (tasks/test-registry-leak-
    /// cleanup): lists them, deleting only with `--yes`. Never touches a root that still exists,
    /// unlike a plain `remove`, which un-registers by id regardless.
    Prune {
        /// Actually remove the dead registrations; omitted is a dry run (list only).
        #[arg(long)]
        yes: bool,
    },
}

/// Entry: `txtodo workspace` with no subcommand is `list` (matches `device`'s own convenience).
pub fn run(
    ctx: &Ctx,
    daemon: &mut Daemon,
    action: Option<&Action>,
    as_json: bool,
) -> Result<(), CliError> {
    match action {
        None | Some(Action::List) => run_list(daemon, as_json),
        Some(Action::Add { dir }) => run_add(ctx, daemon, dir.as_deref(), as_json),
        Some(Action::Remove { id }) => run_remove(daemon, id, as_json),
        Some(Action::Prune { yes }) => run_prune(daemon, *yes, as_json),
        Some(Action::Offers) => super::workspace_offers::run_offers(daemon, as_json),
        Some(Action::Accept { id, from }) => {
            super::workspace_offers::run_accept(daemon, id, from.as_deref(), as_json)
        }
        Some(Action::Decline { id, from }) => {
            super::workspace_offers::run_decline(daemon, id, from.as_deref(), as_json)
        }
        Some(Action::Default) => run_default(&Env::from_process().map_err(CliError::Io)?, as_json),
        Some(Action::Layout {
            refs_dir,
            todo_file,
            move_dirs,
        }) => super::layout::run(
            daemon,
            super::layout::Change {
                refs_dir: refs_dir.as_deref(),
                todo_file: todo_file.as_deref(),
                move_dirs: *move_dirs,
            },
            as_json,
        ),
    }
}

/// `workspace default`: the path, and whether the directory exists yet. Resolved through
/// `config::default_workspace_dir`, which is the daemon's own function, so the two cannot disagree.
pub fn run_default(env: &Env, as_json: bool) -> Result<(), CliError> {
    let dir = crate::config::default_workspace_dir(env);
    if as_json {
        println!(
            r#"{{"path":{},"exists":{}}}"#,
            json::str(&dir.display().to_string()),
            dir.is_dir()
        );
    } else {
        println!("{}", dir.display());
    }
    Ok(())
}

/// How far the daemon is through opening this workspace (task `daemon-early-bind`): `queued`,
/// `loading`, `ready`, `failed`, or `unknown` from a daemon that predates the field.
fn load_state_name(w: &pb::WorkspaceInfo) -> &'static str {
    match pb::WorkspaceLoadState::try_from(w.load_state) {
        Ok(pb::WorkspaceLoadState::Queued) => "queued",
        Ok(pb::WorkspaceLoadState::Loading) => "loading",
        Ok(pb::WorkspaceLoadState::Ready) => "ready",
        Ok(pb::WorkspaceLoadState::Failed) => "failed",
        Ok(pb::WorkspaceLoadState::Unspecified) | Err(_) => "unknown",
    }
}

fn info_json(w: &pb::WorkspaceInfo) -> String {
    format!(
        r#"{{"id":{},"root":{},"is_default":{},"is_remote":{},"added_at_ms":{},"root_exists":{},"has_state":{},"load_state":{},"load_error":{}}}"#,
        json::str(&w.workspace_id),
        json::str(&w.root),
        w.is_default,
        w.is_remote,
        w.added_at_ms,
        w.root_exists,
        w.has_state,
        json::str(load_state_name(w)),
        json::str(&w.load_error)
    )
}

fn info_text(w: &pb::WorkspaceInfo) -> String {
    let missing = if w.root_exists { "" } else { " [missing]" };
    let state = if w.has_state { "" } else { " [new]" };
    let load = match load_state_name(w) {
        "queued" => " [queued]",
        "loading" => " [opening]",
        "failed" => " [failed to open]",
        _ => "",
    };
    let default = if w.is_default { " [default]" } else { "" };
    // A mirror of a paired device's workspace (task remote-workspace-mirror): txtodo chose the
    // folder, so say so beside the path.
    let remote = if w.is_remote { " [remote]" } else { "" };
    format!(
        "{}  {}{default}{remote}{missing}{state}{load}",
        w.workspace_id, w.root
    )
}

/// `workspace add [DIR]`: registers `dir` (default: the resolved `--dir`/cwd), never opens it.
fn run_add(
    ctx: &Ctx,
    daemon: &mut Daemon,
    dir: Option<&str>,
    as_json: bool,
) -> Result<(), CliError> {
    let root = dir.map_or_else(|| ctx.paths.dir.display().to_string(), str::to_owned);
    let info = daemon.workspace_add(&root)?;
    println!(
        "{}",
        if as_json {
            info_json(&info)
        } else {
            info_text(&info)
        }
    );
    Ok(())
}

/// `workspace remove <id>`: un-registers by id (no confirmation prompt — unlike `device remove`,
/// this never un-shares synced history with a peer; it only forgets a registry entry).
fn run_remove(daemon: &mut Daemon, id: &str, as_json: bool) -> Result<(), CliError> {
    let removed = daemon.workspace_remove(id)?;
    if as_json {
        println!(r#"{{"removed":{removed}}}"#);
    } else if removed {
        println!("TODO: workspace {id} removed from the registry.");
    } else {
        return Err(CliError::Message(format!(
            "txtodo: no registered workspace {id}."
        )));
    }
    Ok(())
}

/// `workspace list`: one row per registered workspace, oldest first (the daemon's own order).
fn run_list(daemon: &mut Daemon, as_json: bool) -> Result<(), CliError> {
    let workspaces = daemon.workspace_list()?;
    if workspaces.is_empty() {
        if !as_json {
            println!("TODO: no registered workspaces.");
        }
        return Ok(());
    }
    for w in &workspaces {
        println!("{}", if as_json { info_json(w) } else { info_text(w) });
    }
    Ok(())
}

/// `workspace prune [--yes]`: registered workspaces whose root no longer exists (`root_exists` is
/// already computed by `workspace list`'s own `WorkspaceInfo` — see design's "cheap fs::exists"
/// doc), removed from the registry only with `--yes` (dry run by default, same gate as `prune
/// --orphans`). `root_exists == false` on a workspace this device itself just un-mounted (e.g. a
/// removable drive) is indistinguishable from one that's really gone — the same judgment call a
/// human already makes running `remove` by hand; this just finds the candidates faster.
fn run_prune(daemon: &mut Daemon, yes: bool, as_json: bool) -> Result<(), CliError> {
    let dead: Vec<pb::WorkspaceInfo> = daemon
        .workspace_list()?
        .into_iter()
        .filter(|w| !w.root_exists)
        .collect();
    if dead.is_empty() {
        if !as_json {
            println!("TODO: no dead workspace registrations.");
        }
        return Ok(());
    }
    if yes {
        for w in &dead {
            daemon.workspace_remove(&w.workspace_id)?;
        }
    }
    if as_json {
        for w in &dead {
            println!("{}", info_json(w));
        }
        return Ok(());
    }
    for w in &dead {
        println!("{}", info_text(w));
    }
    if yes {
        println!(
            "TODO: removed {} dead workspace registration(s).",
            dead.len()
        );
    } else {
        println!("TODO: run with --yes to remove.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(state: pb::WorkspaceLoadState) -> pb::WorkspaceInfo {
        pb::WorkspaceInfo {
            workspace_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            root: "/home/a/project".into(),
            root_exists: true,
            has_state: true,
            load_state: state as i32,
            ..pb::WorkspaceInfo::default()
        }
    }

    /// Task `daemon-early-bind`: `scripts/cold-boot-timing.sh` reads `load_state` from this JSON.
    #[test]
    fn json_carries_the_load_state_and_text_marks_only_workspaces_still_opening() {
        assert!(
            info_json(&info(pb::WorkspaceLoadState::Ready)).contains(r#""load_state":"ready""#)
        );
        assert!(
            info_json(&info(pb::WorkspaceLoadState::Loading)).contains(r#""load_state":"loading""#)
        );
        assert!(info_json(&pb::WorkspaceInfo::default()).contains(r#""load_state":"unknown""#));
        assert!(!info_text(&info(pb::WorkspaceLoadState::Ready)).contains('['));
        assert!(info_text(&info(pb::WorkspaceLoadState::Loading)).ends_with("[opening]"));
        assert!(info_text(&info(pb::WorkspaceLoadState::Queued)).ends_with("[queued]"));
        assert!(info_text(&info(pb::WorkspaceLoadState::Failed)).ends_with("[failed to open]"));
    }

    /// Task `remote-workspace-mirror`: a mirrored workspace says so in both forms.
    #[test]
    fn a_mirrored_workspace_is_marked_remote() {
        let mirror = pb::WorkspaceInfo {
            is_remote: true,
            ..info(pb::WorkspaceLoadState::Ready)
        };
        assert!(info_text(&mirror).contains("[remote]"));
        assert!(info_json(&mirror).contains(r#""is_remote":true"#));
        assert!(!info_text(&info(pb::WorkspaceLoadState::Ready)).contains("[remote]"));
    }
}
