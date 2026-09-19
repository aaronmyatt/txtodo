//! `txtodo workspace add|remove|list` (ADR 0025, task `cli-workspace-commands`): manages the
//! device-global daemon's workspace registry. Needs the true global daemon — a legacy
//! `--dir`-bridge daemon has no registry to answer these with (`Daemon::workspace_add`/etc.
//! return an `Unimplemented` `ClientError::Rpc` against one, surfaced as a normal error).

use crate::client::Daemon;
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
    }
}

fn info_json(w: &pb::WorkspaceInfo) -> String {
    format!(
        r#"{{"id":{},"root":{},"added_at_ms":{},"root_exists":{},"has_state":{}}}"#,
        json::str(&w.workspace_id),
        json::str(&w.root),
        w.added_at_ms,
        w.root_exists,
        w.has_state
    )
}

fn info_text(w: &pb::WorkspaceInfo) -> String {
    let missing = if w.root_exists { "" } else { " [missing]" };
    let state = if w.has_state { "" } else { " [new]" };
    format!("{}  {}{missing}{state}", w.workspace_id, w.root)
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
