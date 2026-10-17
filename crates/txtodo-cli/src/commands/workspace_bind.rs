//! `txtodo workspace bind <id> [DIR] [--yes] [--dry-run]` (task
//! spike-txtodo-should-merge-remote-workspa): moves a Remote mirror into a folder, for a repo
//! cloned on two devices. The daemon does the work (`WorkspaceBind`); this asks it for a dry run
//! first, so a refusal (not a mirror, a nested folder, another disk) comes before any prompt, then
//! shows what moves each way and asks. The prompt defaults to no, and a closed stdin is a no.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::client::Daemon;
use crate::config::Env;
use crate::{CliError, Ctx, json};
use txtodo_proto::v1 as pb;

/// How many moved paths the prompt names, each way, before it counts the rest.
const SHOWN: usize = 5;

/// `workspace bind`'s arguments.
/// Ref: https://docs.rs/clap/latest/clap/_derive/index.html#flattening-hoisting-and-variants
#[derive(Debug, clap::Args)]
pub struct Args {
    /// The Remote workspace's id (ULID text, from `workspace list`).
    pub id: String,
    /// The folder; defaults to this workspace's folder (or, outside one, the current folder).
    pub dir: Option<String>,
    /// Skip the confirmation prompt.
    #[arg(long)]
    pub yes: bool,
    /// Only show what would move.
    #[arg(long)]
    pub dry_run: bool,
}

/// `workspace bind`.
pub fn run(ctx: &Ctx, daemon: &mut Daemon, args: &Args, as_json: bool) -> Result<(), CliError> {
    let folder = folder(ctx, args.dir.as_deref())?;
    let root = folder.display().to_string();
    let plan = daemon.workspace_bind(&args.id, &root, true)?;
    if args.dry_run {
        print_result(&plan, &root, as_json, true);
        return Ok(());
    }
    if !args.yes && !confirm(&plan, &root)? {
        return Err(CliError::Message(
            "txtodo: bind not confirmed; nothing changed.".to_owned(),
        ));
    }
    let done = daemon.workspace_bind(&args.id, &root, false)?;
    print_result(&done, &root, as_json, false);
    Ok(())
}

/// `DIR` when given; else the resolved `--dir`/cwd workspace, unless that fell back to the default
/// workspace (the folder holds no list yet): then the folder the command ran in.
fn folder(ctx: &Ctx, dir: Option<&str>) -> Result<PathBuf, CliError> {
    if let Some(dir) = dir {
        return Ok(PathBuf::from(dir));
    }
    let env = Env::from_process().map_err(CliError::Io)?;
    let default = crate::config::default_workspace_dir(&env);
    if same_dir(&ctx.paths.dir, &default) {
        // Ref: https://doc.rust-lang.org/std/env/fn.current_dir.html
        return std::env::current_dir().map_err(CliError::Io);
    }
    Ok(ctx.paths.dir.clone())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    // Ref: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn print_result(r: &pb::WorkspaceBindResponse, root: &str, as_json: bool, dry_run: bool) {
    if as_json {
        println!("{}", result_json(r, root, dry_run));
    } else if dry_run {
        print!("{}", plan_text(r, root));
    } else {
        println!("{}", done_text(r, root));
    }
}

/// The prompt, on stderr so `--json` output stays clean; `true` only for `y` or `yes`.
fn confirm(plan: &pb::WorkspaceBindResponse, root: &str) -> Result<bool, CliError> {
    eprint!("{}Continue? [y/N] ", plan_text(plan, root));
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(CliError::Io)?;
    Ok(is_yes(&line))
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn name_of(r: &pb::WorkspaceBindResponse) -> String {
    let Some(w) = &r.workspace else {
        return "the Remote workspace".to_owned();
    };
    match w.name.is_empty() {
        true => format!("Remote workspace {}", w.workspace_id),
        false => format!("Remote workspace {} ({})", w.name, w.workspace_id),
    }
}

fn plan_text(plan: &pb::WorkspaceBindResponse, root: &str) -> String {
    let mut text = format!("Move {} into {root}?\n", name_of(plan));
    if plan.moved_aside.is_empty() {
        text.push_str("This folder has no list of its own; nothing moves out.\n");
    } else {
        text.push_str(&format!(
            "This folder's own copy moves into {} (nothing is deleted or sent):\n",
            plan.backup_dir
        ));
        push_paths(&mut text, &plan.moved_aside);
    }
    text.push_str("The synced copy moves in:\n");
    push_paths(&mut text, &plan.moved_in);
    if !plan.replaced_workspace_id.is_empty() {
        text.push_str(&format!(
            "Workspace {} is un-registered here; it replaces it.\n",
            plan.replaced_workspace_id
        ));
    }
    text
}

fn push_paths(text: &mut String, paths: &[String]) {
    for path in paths.iter().take(SHOWN) {
        text.push_str(&format!("  {path}\n"));
    }
    if paths.len() > SHOWN {
        text.push_str(&format!("  … and {} more\n", paths.len() - SHOWN));
    }
}

fn done_text(done: &pb::WorkspaceBindResponse, root: &str) -> String {
    let mut text = format!("TODO: {root} now holds {}.", name_of(done));
    if !done.backup_dir.is_empty() {
        text.push_str(&format!(" Its own copy is in {}.", done.backup_dir));
    }
    if !done.replaced_workspace_id.is_empty() {
        let old = &done.replaced_workspace_id;
        text.push_str(&format!(
            "\nOther devices may still show workspace {old} as Remote; remove it there with \
             `txtodo workspace remove {old}`."
        ));
    }
    text
}

fn result_json(r: &pb::WorkspaceBindResponse, root: &str, dry_run: bool) -> String {
    let list = |items: &[String]| json::strs(items.iter().map(String::as_str));
    let id = r.workspace.as_ref().map_or("", |w| w.workspace_id.as_str());
    format!(
        r#"{{"id":{},"root":{},"dry_run":{dry_run},"backup":{},"moved_aside":{},"moved_in":{},"replaced":{}}}"#,
        json::str(id),
        json::str(root),
        json::str(&r.backup_dir),
        list(&r.moved_aside),
        list(&r.moved_in),
        json::str(&r.replaced_workspace_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(aside: usize, replaced: &str) -> pb::WorkspaceBindResponse {
        let paths = |n: usize| (0..n).map(|i| format!("doc{i}/todo.txt")).collect();
        pb::WorkspaceBindResponse {
            workspace: Some(pb::WorkspaceInfo {
                workspace_id: "01M3ZKQNERX09CT41RBFJVE3AC".into(),
                name: "txtodo".into(),
                ..pb::WorkspaceInfo::default()
            }),
            backup_dir: match aside {
                0 => String::new(),
                _ => "/home/a/txtodo.rejoin-backup-2026-10-17T120000Z".into(),
            },
            moved_aside: paths(aside),
            moved_in: paths(2),
            replaced_workspace_id: replaced.into(),
        }
    }

    #[test]
    fn only_y_or_yes_confirms() {
        for yes in ["y\n", "Y", " yes \n"] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", "\n", "n", "nope"] {
            assert!(!is_yes(no), "{no:?}");
        }
    }

    #[test]
    fn the_plan_names_the_mirror_both_moves_and_the_workspace_it_replaces() {
        let text = plan_text(&plan(7, "01M4MJ4YSYDGEBJWYB3F85B3DP"), "/home/a/txtodo");
        assert!(text.starts_with(
            "Move Remote workspace txtodo (01M3ZKQNERX09CT41RBFJVE3AC) into /home/a/txtodo?\n"
        ));
        assert!(text.contains("moves into /home/a/txtodo.rejoin-backup-2026-10-17T120000Z"));
        assert!(text.contains("  doc4/todo.txt\n  … and 2 more\nThe synced copy moves in:\n"));
        assert!(text.contains("Workspace 01M4MJ4YSYDGEBJWYB3F85B3DP is un-registered here"));
        let empty = plan_text(&plan(0, ""), "/home/a/new");
        assert!(empty.contains("no list of its own"), "{empty}");
        assert!(!empty.contains("un-registered"), "{empty}");
    }

    #[test]
    fn done_says_where_each_copy_is_and_json_carries_both_lists() {
        let text = done_text(&plan(1, "01M4MJ4YSYDGEBJWYB3F85B3DP"), "/home/a/txtodo");
        assert!(text.contains("/home/a/txtodo now holds Remote workspace txtodo"));
        assert!(text.contains("Its own copy is in /home/a/txtodo.rejoin-backup-"));
        assert!(text.contains("`txtodo workspace remove 01M4MJ4YSYDGEBJWYB3F85B3DP`"));
        let json = result_json(&plan(1, ""), "/home/a/txtodo", true);
        assert!(json.contains(r#""dry_run":true"#), "{json}");
        assert!(
            json.contains(r#""moved_aside":["doc0/todo.txt"]"#),
            "{json}"
        );
        assert!(json.contains(r#""moved_in":["doc0/todo.txt","doc1/todo.txt"]"#));
        assert!(json.contains(r#""replaced":"""#), "{json}");
    }
}
