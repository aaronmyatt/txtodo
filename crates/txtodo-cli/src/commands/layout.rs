//! `txtodo workspace layout` and the `doctor` rows about it (task `workspace-layout`): where a
//! workspace keeps its root list and the folder for its `ref:` lines. The daemon owns the answer
//! and writes `<root>/txtodo.toml`; this only asks and prints.

use super::doctor::{Check, Status, check};
use crate::client::Daemon;
use crate::{CliError, Ctx, json};
use txtodo_proto::v1 as pb;

/// What `workspace layout` was asked to change; all `None` and `false` means "just show it".
pub struct Change<'a> {
    pub refs_dir: Option<&'a str>,
    pub todo_file: Option<&'a str>,
    pub move_dirs: bool,
}

/// `workspace layout [--refs-dir P] [--todo-file P] [--move]`.
pub fn run(daemon: &mut Daemon, change: Change<'_>, as_json: bool) -> Result<(), CliError> {
    let set = change.refs_dir.is_some() || change.todo_file.is_some();
    let info = daemon.workspace_layout(pb::WorkspaceLayoutRequest {
        set,
        refs_dir: change.refs_dir.unwrap_or_default().to_owned(),
        todo_file: change.todo_file.unwrap_or_default().to_owned(),
        move_dirs: change.move_dirs,
        workspace: None,
    })?;
    if as_json {
        println!(
            r#"{{"refs_dir":{},"todo_file":{},"note":{},"moved":{},"outside_refs_dir":{}}}"#,
            json::str(&info.refs_dir),
            json::str(&info.todo_file),
            json::str(&info.note),
            info.moved,
            json::strs(info.outside_refs_dir.iter().map(String::as_str))
        );
        return Ok(());
    }
    println!("refs_dir  = {}", info.refs_dir);
    println!("todo_file = {}", info.todo_file);
    if info.moved > 0 {
        println!("TODO: moved {} ref dir(s).", info.moved);
    }
    if !info.note.is_empty() {
        println!("note: {}", info.note);
    }
    for dir in &info.outside_refs_dir {
        println!("outside refs_dir: {dir}");
    }
    Ok(())
}

/// The default workspace's directory and the layout, as `doctor` rows; never a FAIL.
pub fn doctor_checks(ctx: &Ctx, daemon: Option<&mut Daemon>) -> Vec<Check> {
    let mut checks = vec![default_workspace_check(ctx)];
    if let Some(Ok(info)) =
        daemon.map(|d| d.workspace_layout(pb::WorkspaceLayoutRequest::default()))
    {
        checks.push(layout_check(&info));
    }
    checks
}

/// Where the default workspace lives (Finder will not show it); never a FAIL, the daemon makes it.
fn default_workspace_check(ctx: &Ctx) -> Check {
    let dir = &ctx.paths.default_dir;
    let state = if dir.join("todo.txt").is_file() {
        "exists"
    } else {
        "not created yet; the daemon makes it on start"
    };
    check(
        "default",
        Status::Ok,
        format!("{} ({state})", dir.display()),
    )
}

/// The layout in force, a warning when the file and the layout disagree, and any ref dir left
/// outside `refs_dir` (`prune` will not touch those).
fn layout_check(info: &pb::WorkspaceLayoutInfo) -> Check {
    let mut detail = format!(
        "refs_dir = {}, todo_file = {}",
        info.refs_dir, info.todo_file
    );
    let mut status = Status::Ok;
    if !info.note.is_empty() {
        status = Status::Warn;
        detail.push_str(&format!("; {}", info.note));
    }
    if !info.outside_refs_dir.is_empty() {
        status = Status::Warn;
        detail.push_str(&format!(
            "; outside refs_dir: {}",
            info.outside_refs_dir.join(", ")
        ));
    }
    check("layout", status, detail)
}
