//! Duplicate lines in `txtodo conflicts` (ADR 0032, task sync-drift/duplicate-flags): two or more
//! tasks in one file whose whole lines are the same bytes. The daemon derives the groups from the
//! file (`ConflictsResponse.duplicates`); this lists them and resolves them with the `Apply`
//! `Delete` that already exists, after a confirm. Keeping both copies means editing one so they
//! differ: there is no dismiss. Split out of `conflicts.rs` for its file budget.

use std::io::Write;

use crate::client::Daemon;
use crate::{CliError, json};
use txtodo_proto::v1 as pb;

/// The groups as listed: each copy's line and text, oldest id first.
pub(super) fn print_groups(groups: &[pb::DuplicateGroup], lines: &[String], as_json: bool) {
    for group in groups {
        let text = text_of(group, lines);
        if as_json {
            println!("{}", group_json(group, text));
        } else {
            println!("{}", group_text(group, text));
        }
    }
    if !as_json && !groups.is_empty() {
        println!(
            "TODO: delete one copy with `txtodo conflicts delete <line>`, keep the newest of each \
             with `txtodo conflicts keep-newest`, or edit one so they differ."
        );
    }
}

/// `conflicts delete <line>`: one copy of a duplicated line, after a confirm. Refuses a line that
/// is in no group: this is not a general delete.
pub(super) fn run_delete(
    daemon: &mut Daemon,
    file: &str,
    line: u32,
    yes: bool,
) -> Result<(), CliError> {
    let groups = daemon.conflicts(file)?.duplicates;
    let Some(copy) = groups
        .iter()
        .flat_map(|g| &g.tasks)
        .find(|t| t.line_number == line)
    else {
        eprintln!("TODO: line {line} is not a duplicate; `txtodo conflicts` lists them.");
        return Err(CliError::Reported);
    };
    if !yes && !confirm(&format!("Delete line {line}? The other copy stays."))? {
        println!("TODO: nothing deleted.");
        return Ok(());
    }
    daemon.apply(file, vec![delete(copy)])?;
    println!("TODO: {line} deleted.");
    Ok(())
}

/// `conflicts keep-newest`: in every group of the file, every copy but the one with the latest
/// id, in one `Apply` after one confirm. The latest id is the safe default (ADR 0032): a device
/// that re-minted its lines holds only the new id, so deleting the old one there is a no-op.
pub(super) fn run_keep_newest(daemon: &mut Daemon, file: &str, yes: bool) -> Result<(), CliError> {
    let groups = daemon.conflicts(file)?.duplicates;
    let older = older_copies(&groups);
    if older.is_empty() {
        println!("TODO: no duplicate lines.");
        return Ok(());
    }
    let question = format!(
        "Delete {} older cop{} in {} group{}? The newest of each stays.",
        older.len(),
        if older.len() == 1 { "y" } else { "ies" },
        groups.len(),
        if groups.len() == 1 { "" } else { "s" }
    );
    if !yes && !confirm(&question)? {
        println!("TODO: nothing deleted.");
        return Ok(());
    }
    let applied = daemon.apply(file, older.into_iter().map(delete).collect())?;
    println!("TODO: {} deleted.", applied.applied);
    Ok(())
}

/// Every copy but the newest (the last: the daemon lists oldest id first) in each group.
fn older_copies(groups: &[pb::DuplicateGroup]) -> Vec<&pb::DuplicateTask> {
    groups
        .iter()
        .flat_map(|g| g.tasks.split_last().map_or(&[][..], |(_, older)| older))
        .collect()
}

/// A `Delete` by task id alone: one batch deletes several lines, so line numbers would move under
/// it, while the id names the copy whatever line it is on.
fn delete(copy: &pb::DuplicateTask) -> pb::Mutation {
    pb::Mutation {
        kind: Some(pb::mutation::Kind::Delete(pb::Delete {
            task: Some(pb::TaskRef {
                line_number: 0,
                task_id: copy.task_id.clone(),
            }),
            leave_blank: false,
        })),
    }
}

/// `[y/N]` on stderr, so `--json` output stays clean; only `y` or `yes` is a yes, and a closed
/// stdin is a no (the same rule as `workspace rejoin`).
fn confirm(question: &str) -> Result<bool, CliError> {
    eprint!("{question} [y/N] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(CliError::Io)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// The line every copy shares, read from the file at the first copy's line.
fn text_of<'a>(group: &pb::DuplicateGroup, lines: &'a [String]) -> &'a str {
    group
        .tasks
        .first()
        .and_then(|t| usize::try_from(t.line_number).ok()?.checked_sub(1))
        .and_then(|i| lines.get(i))
        .map_or("", String::as_str)
}

fn group_text(group: &pb::DuplicateGroup, text: &str) -> String {
    let last = group.tasks.len().saturating_sub(1);
    let mut out = format!(
        "TODO: duplicate line ({} copies): {text}",
        group.tasks.len()
    );
    for (i, t) in group.tasks.iter().enumerate() {
        let age = match i {
            0 => " (oldest)",
            i if i == last => " (newest)",
            _ => "",
        };
        out.push_str(&format!("\n  line {}{age}", t.line_number));
    }
    out
}

/// One JSON object per group, like every listing command (`--json`).
fn group_json(group: &pb::DuplicateGroup, text: &str) -> String {
    let lines: Vec<String> = group
        .tasks
        .iter()
        .map(|t| t.line_number.to_string())
        .collect();
    let ids: Vec<String> = group.tasks.iter().map(|t| json::str(&t.task_id)).collect();
    format!(
        r#"{{"duplicate":{text},"lines":[{lines}],"task_ids":[{ids}]}}"#,
        text = json::str(text),
        lines = lines.join(","),
        ids = ids.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(copies: &[(&str, u32)]) -> pb::DuplicateGroup {
        pb::DuplicateGroup {
            tasks: copies
                .iter()
                .map(|(id, line)| pb::DuplicateTask {
                    task_id: (*id).to_owned(),
                    line_number: *line,
                })
                .collect(),
        }
    }

    #[test]
    fn keep_newest_deletes_every_copy_but_the_last_of_each_group() {
        let groups = [
            group(&[("A", 1), ("B", 4)]),
            group(&[("C", 2), ("D", 3), ("E", 5)]),
        ];
        let older: Vec<&str> = older_copies(&groups)
            .iter()
            .map(|t| t.task_id.as_str())
            .collect();
        assert_eq!(older, ["A", "C", "D"]);
        assert!(older_copies(&[]).is_empty());
    }

    #[test]
    fn a_group_lists_each_line_with_oldest_and_newest_marked() {
        let lines = [
            "buy milk".to_owned(),
            "walk".to_owned(),
            "buy milk".to_owned(),
        ];
        let g = group(&[("A", 1), ("B", 3)]);
        assert_eq!(
            group_text(&g, text_of(&g, &lines)),
            "TODO: duplicate line (2 copies): buy milk\n  line 1 (oldest)\n  line 3 (newest)"
        );
        assert_eq!(
            group_json(&g, text_of(&g, &lines)),
            r#"{"duplicate":"buy milk","lines":[1,3],"task_ids":["A","B"]}"#
        );
    }
}
