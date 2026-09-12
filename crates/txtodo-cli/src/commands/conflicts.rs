//! `txtodo conflicts [list]` and `txtodo conflicts resolve <line> <side>` (plan M4): two devices
//! rewrote the same word offline and the daemon raised a needs_review flag. Both subcommands need
//! the daemon — a flag lives in the daemon's store, never in the file: a flagged file is
//! byte-identical to an unflagged one, so direct-file mode cannot see conflicts at all.
//! "mine" is this device's text at flag time, "theirs" the peer's (the reading recorded in
//! tasks/crdt-needs-review/notes.md); "merged" keeps what is in the file and writes nothing.

use crate::client::Daemon;
use crate::{CliError, json};
use clap::Subcommand;
use txtodo_core::{LineKind, parse_file};
use txtodo_proto::v1 as pb;

/// The `txtodo conflicts` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Open flags, one block per task: the line, then a unified diff of the two texts.
    #[command(visible_alias = "ls")]
    List {
        /// Which document (workspace-relative).
        #[arg(long, default_value = "todo.txt")]
        file: String,
    },
    /// Keep one side of a conflicted task and clear its flag — both or neither.
    Resolve {
        /// The line the task sits on now.
        line: u32,
        /// Which side to keep: this device's text, the peer's, or the file as merged.
        side: Side,
        /// Which document (workspace-relative).
        #[arg(long, default_value = "todo.txt")]
        file: String,
    },
}

/// Which side wins. A closed clap enum, never a free string, so a typo is a usage error and not
/// a surprise at resolve time. Mirrors the wire enum `pb::Resolution`.
/// Ref: https://docs.rs/clap/latest/clap/trait.ValueEnum.html
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Side {
    /// This device's text at flag time.
    Mine,
    /// The peer's text at flag time.
    Theirs,
    /// What is in the file now; only the flag is cleared, nothing is written.
    Merged,
}

impl Side {
    /// The lowercase word for messages, matching the CLI argument.
    fn word(self) -> &'static str {
        match self {
            Side::Mine => "mine",
            Side::Theirs => "theirs",
            Side::Merged => "merged",
        }
    }
}

/// The wire enum for a side (proto `Resolution`; same closed set).
fn wire(side: Side) -> pb::Resolution {
    match side {
        Side::Mine => pb::Resolution::Mine,
        Side::Theirs => pb::Resolution::Theirs,
        Side::Merged => pb::Resolution::Merged,
    }
}

/// Entry: `txtodo conflicts` with no subcommand is `list` (notes: an alias for muscle memory).
pub fn run(daemon: &mut Daemon, action: Option<&Action>, as_json: bool) -> Result<(), CliError> {
    match action {
        None => run_list(daemon, "todo.txt", as_json),
        Some(Action::List { file }) => run_list(daemon, file, as_json),
        Some(Action::Resolve { line, side, file }) => run_resolve(daemon, *line, *side, file),
    }
}

/// `conflicts list`: one block per open flag, or the todo.sh-style "nothing to report" line.
pub fn run_list(daemon: &mut Daemon, file: &str, as_json: bool) -> Result<(), CliError> {
    let flags = daemon.conflicts(file)?;
    if flags.is_empty() {
        // JSON mode prints nothing: absence of lines means "none", and scripts grep objects.
        if !as_json {
            println!("TODO: no conflicts.");
        }
        return Ok(());
    }
    let lines = current_lines(daemon, file)?;
    debug_assert!(
        flags
            .iter()
            .all(|f| usize::try_from(f.line_number).unwrap_or(0) <= lines.len()),
        "line_number is 0 (gone) or a real 1-based line"
    );
    for f in &flags {
        // 0 means the task left the file; otherwise it is a 1-based line over every line.
        let n = usize::try_from(f.line_number).unwrap_or(0);
        let current = n
            .checked_sub(1)
            .and_then(|i| lines.get(i))
            .map_or_else(|| "(no longer in the file)", |s| s.as_str());
        println!(
            "{}",
            if as_json {
                flag_json(f, current)
            } else {
                flag_text(f, current)
            }
        );
    }
    Ok(())
}

/// `conflicts resolve <line> <side>`: the daemon writes the chosen text and clears the flag in
/// one store transaction (both or neither); `merged` clears only and writes no op.
pub fn run_resolve(daemon: &mut Daemon, line: u32, side: Side, file: &str) -> Result<(), CliError> {
    let bytes = daemon.get(file)?;
    let doc = parse_file(&bytes);
    // Send both addresses when the line has an id; the daemon then rejects a stale line number
    // (proto `TaskRef`: "the daemon rejects the mutation when they disagree").
    let kind = usize::try_from(line)
        .unwrap_or(0)
        .checked_sub(1)
        .and_then(|i| doc.lines.get(i))
        .and_then(|l| l.parse())
        .map(|l| l.kind);
    let task_id = match kind {
        Some(LineKind::Task(t)) => t.id().map(|u| u.to_string()),
        _ => None,
    }
    .unwrap_or_default();
    let rep = daemon.resolve_conflict(pb::ResolveRequest {
        path: file.to_owned(),
        task: Some(pb::TaskRef {
            line_number: line,
            task_id,
        }),
        resolution: wire(side).into(),
    })?;
    // `merged` writes no op, so a non-zero count here means the daemon did the wrong thing.
    debug_assert!(
        !matches!(side, Side::Merged) || rep.applied == 0,
        "merged clears the flag and writes nothing"
    );
    println!(
        "TODO: resolved the conflict on line {line} ({}).",
        side.word()
    );
    Ok(())
}

/// The file's lines as they are now, for the "what is in the file" row of `list`.
fn current_lines(daemon: &mut Daemon, file: &str) -> Result<Vec<String>, CliError> {
    let bytes = daemon.get(file)?;
    Ok(parse_file(&bytes)
        .lines
        .iter()
        // An opaque (non-UTF-8) line cannot be a task; show it for what it is.
        .map(|l| l.raw().unwrap_or("(not UTF-8)").to_owned())
        .collect())
}

/// One text block: the header names where the task sits now and what the file says; the body is
/// a two-line unified diff. A todo.txt description is one line, so the diff is a single hunk of
/// `-mine` `+theirs` (diff(1) format:
/// https://www.gnu.org/software/diffutils/manual/html_node/Unified-Format.html).
fn flag_text(f: &pb::ReviewFlag, current: &str) -> String {
    let where_now = if f.line_number == 0 {
        "gone from the file".to_owned()
    } else {
        format!("line {}", f.line_number)
    };
    format!(
        "TODO: task needs review ({where_now}): {current}\n\
         --- mine (this device)\n\
         +++ theirs (the other device)\n\
         - {}\n\
         + {}",
        f.mine, f.theirs
    )
}

/// One JSON object per line, like every listing command (`--json`).
fn flag_json(f: &pb::ReviewFlag, current: &str) -> String {
    format!(
        r#"{{"line":{line},"current":{current},"mine":{mine},"theirs":{theirs},"raised_at_ms":{at}}}"#,
        line = f.line_number,
        current = json::str(current),
        mine = json::str(&f.mine),
        theirs = json::str(&f.theirs),
        at = f.raised_at_ms
    )
}

/// Unit tests live in the sibling `conflicts_tests.rs` (the repo's `*_tests.rs` precedent); a child
/// module, so the render helpers stay private to this file.
#[cfg(test)]
#[path = "conflicts_tests.rs"]
mod tests;
