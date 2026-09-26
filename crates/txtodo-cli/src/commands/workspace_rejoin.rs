//! `txtodo workspace rejoin <id> [--yes]` (task sync-drift line 8): drops this device's copy of
//! one workspace and takes a paired device's. The daemon does the work (`WorkspaceRejoin`); this
//! asks it for a dry run first, so a refusal (no paired device offers the workspace, the default,
//! a nested workspace) comes before any prompt, then shows what would move and asks. The prompt
//! defaults to no, and a closed stdin is a no. Split out of `workspace.rs` for its file budget.

use std::collections::BTreeMap;
use std::io::Write;

use crate::client::Daemon;
use crate::{CliError, json};
use txtodo_proto::v1 as pb;

/// How many moved paths the prompt names before it counts the rest.
const SHOWN: usize = 5;

/// `workspace rejoin <id> [--yes]`.
pub fn run(daemon: &mut Daemon, id: &str, yes: bool, as_json: bool) -> Result<(), CliError> {
    let plan = daemon.workspace_rejoin(id, true)?;
    let names = device_names(daemon);
    if !yes && !confirm(&plan, &names)? {
        return Err(CliError::Message(
            "txtodo: rejoin not confirmed; nothing changed.".to_owned(),
        ));
    }
    let done = daemon.workspace_rejoin(id, false)?;
    if as_json {
        println!("{}", done_json(&done));
    } else {
        println!("{}", done_text(&done, &names));
    }
    Ok(())
}

/// Paired devices' names by id, for the messages; empty when the daemon cannot say.
fn device_names(daemon: &mut Daemon) -> BTreeMap<String, String> {
    let devices = daemon.device_list().unwrap_or_default();
    devices
        .into_iter()
        .filter(|d| !d.name.is_empty())
        .map(|d| (d.id, d.name))
        .collect()
}

/// The newest offering device, by name when known.
fn peer(plan: &pb::WorkspaceRejoinResponse, names: &BTreeMap<String, String>) -> String {
    let Some(id) = plan.offering_devices.first() else {
        return "a paired device".to_owned();
    };
    match names.get(id) {
        Some(name) => format!("{name} ({id})"),
        None => format!("device {id}"),
    }
}

fn root(plan: &pb::WorkspaceRejoinResponse) -> &str {
    plan.workspace.as_ref().map_or("", |w| w.root.as_str())
}

/// The prompt, on stderr so `--json` output stays clean; `true` only for `y` or `yes`.
fn confirm(
    plan: &pb::WorkspaceRejoinResponse,
    names: &BTreeMap<String, String>,
) -> Result<bool, CliError> {
    eprint!("{}", prompt_text(plan, names));
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

fn prompt_text(plan: &pb::WorkspaceRejoinResponse, names: &BTreeMap<String, String>) -> String {
    let mut text = format!(
        "Drop this device's copy of {} and take {}'s?\nMoves into {} (nothing is deleted or \
         sent):\n",
        root(plan),
        peer(plan, names),
        plan.backup_dir
    );
    for path in plan.moved.iter().take(SHOWN) {
        text.push_str(&format!("  {path}\n"));
    }
    if plan.moved.len() > SHOWN {
        text.push_str(&format!("  … and {} more\n", plan.moved.len() - SHOWN));
    }
    text.push_str("Continue? [y/N] ");
    text
}

fn done_text(done: &pb::WorkspaceRejoinResponse, names: &BTreeMap<String, String>) -> String {
    format!(
        "TODO: this device's copy of {} is in {} ({} moved). {}'s copy fills it on the next \
         sync; don't edit it here until its lines show up.",
        root(done),
        done.backup_dir,
        done.moved.len(),
        peer(done, names)
    )
}

fn done_json(done: &pb::WorkspaceRejoinResponse) -> String {
    let list = |items: &[String]| json::strs(items.iter().map(String::as_str));
    let id = done
        .workspace
        .as_ref()
        .map_or("", |w| w.workspace_id.as_str());
    format!(
        r#"{{"id":{},"root":{},"backup":{},"moved":{},"offering_devices":{}}}"#,
        json::str(id),
        json::str(root(done)),
        json::str(&done.backup_dir),
        list(&done.moved),
        list(&done.offering_devices)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(moved: usize) -> pb::WorkspaceRejoinResponse {
        pb::WorkspaceRejoinResponse {
            workspace: Some(pb::WorkspaceInfo {
                workspace_id: "01M2RZ8EX1CQAS21TNZ5YY6PBT".into(),
                root: "/home/a/todo".into(),
                ..pb::WorkspaceInfo::default()
            }),
            backup_dir: "/home/a/todo.rejoin-backup-2026-09-26T150211Z".into(),
            moved: (0..moved).map(|n| format!("doc{n}/todo.txt")).collect(),
            offering_devices: vec!["01ARZ3NDEKTSV4RRFFQ69G5FAV".into()],
        }
    }

    #[test]
    fn only_y_or_yes_confirms() {
        for yes in ["y\n", "Y", " yes \n", "YES"] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", "\n", "n", "no", "yep", "sure"] {
            assert!(!is_yes(no), "{no:?}");
        }
    }

    #[test]
    fn the_prompt_names_the_peer_the_backup_and_what_moves_and_defaults_to_no() {
        let names =
            BTreeMap::from([("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(), "laptop".to_owned())]);
        let text = prompt_text(&plan(7), &names);
        assert!(text.contains("of /home/a/todo and take laptop (01ARZ3NDEKTSV4RRFFQ69G5FAV)'s?"));
        assert!(text.contains("todo.rejoin-backup-2026-09-26T150211Z (nothing is deleted"));
        assert!(text.contains("  doc4/todo.txt\n  … and 2 more\n"), "{text}");
        assert!(text.ends_with("[y/N] "));
        assert!(prompt_text(&plan(1), &BTreeMap::new()).contains("device 01ARZ3ND"));
    }

    #[test]
    fn done_says_where_the_copy_went_and_json_carries_every_moved_path() {
        let text = done_text(&plan(2), &BTreeMap::new());
        assert!(text.contains("is in /home/a/todo.rejoin-backup-2026-09-26T150211Z (2 moved)"));
        assert!(text.contains("don't edit it here"), "{text}");
        let json = done_json(&plan(2));
        assert!(json.contains(r#""backup":"/home/a/todo.rejoin-backup-2026-09-26T150211Z""#));
        assert!(
            json.contains(r#""moved":["doc0/todo.txt","doc1/todo.txt"]"#),
            "{json}"
        );
        assert!(json.contains(r#""offering_devices":["01ARZ3NDEKTSV4RRFFQ69G5FAV"]"#));
    }
}
