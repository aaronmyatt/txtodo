//! Daemon mode for every todo.sh command without rewriting them: run the direct-file command
//! against a scratch copy of the daemon's bytes, then express the resulting diff as intent-level
//! mutations and `Apply` them. Output stays byte-identical to direct mode because the same code
//! prints it. `archive`'s reorder goes out as guarded `MoveToEnd` mutations (`archive_plan.rs`). A
//! diff no mutation can express (blank-line removal, mid-file inserts, any other move, every edit
//! in sidecar mode) goes out as one `Replace` of the whole document, naming the hash the command
//! read: the daemon refuses it if the document changed since, instead of overwriting. A mutation
//! batch that addresses a line by number alone (sidecar text has no `id:` to check) leads with a
//! `RequireBase` on the same hash (`base_guard.rs`).

use crate::base_guard::guarded;
use crate::client::Daemon;
use crate::config::Paths;
use crate::plan_check::reproduces;
use crate::{CliError, Ctx, store};
use std::path::Path;
use txtodo_core::{File, LineDiff, LineKind, OwnedLine, diff_lines, parse_file};
use txtodo_proto::v1::{self as pb, mutation};

/// The documents the CLI edits, at the workspace root.
pub const DOCS: [&str; 1] = ["todo.txt"];

/// A document as the daemon held it before the command ran.
struct Original {
    doc: &'static str,
    bytes: Vec<u8>,
    /// The daemon's hash of `bytes`, empty when it does not know the document yet.
    hash: Vec<u8>,
    known: bool,
}

/// Runs `command` against a scratch copy and pushes the changes through the daemon.
pub fn run_via_daemon(
    ctx: &Ctx,
    daemon: &mut Daemon,
    command: impl FnOnce(&Ctx) -> Result<(), CliError>,
) -> Result<(), CliError> {
    let scratch = tempfile::tempdir().map_err(CliError::Io)?;
    let listed: Vec<String> = daemon.list_files()?.into_iter().map(|f| f.path).collect();
    let mut originals: Vec<Original> = Vec::with_capacity(DOCS.len());
    for doc in DOCS {
        let known = listed.iter().any(|k| k == doc);
        let (bytes, hash) = if known {
            daemon.snapshot(doc)?
        } else {
            (Vec::new(), Vec::new())
        };
        if !bytes.is_empty() {
            std::fs::write(scratch.path().join(doc), &bytes).map_err(CliError::Io)?;
        }
        originals.push(Original {
            doc,
            bytes,
            hash,
            known,
        });
    }
    let scratch_ctx = Ctx {
        paths: Paths {
            dir: scratch.path().to_path_buf(),
            todo: scratch.path().join("todo.txt"),
            report: scratch.path().join("report.txt"),
            config: ctx.paths.config.clone(),
            // Sync is a separate, device-global folder, unrelated to this scratch todo-dir copy —
            // carried through unchanged rather than cleared, so a command run in daemon mode sees
            // the same configured sync folder direct mode would.
            sync_dir: ctx.paths.sync_dir.clone(),
            relay_url: ctx.paths.relay_url.clone(),
        },
        config: ctx.config.clone(),
        json: ctx.json,
        ids: ctx.ids,
        auto_archive: ctx.auto_archive,
        today: ctx.today,
    };
    command(&scratch_ctx)?;
    for original in &originals {
        push_document(ctx, daemon, scratch.path(), original)?;
    }
    copy_back(scratch.path(), &ctx.paths.dir, "report.txt")?;
    Ok(())
}

/// Sends one document's diff as mutations, or the whole new document as a guarded `Replace` when
/// no mutation can express the diff. A document the daemon does not know yet is written directly
/// instead (it adopts the file through its watcher).
fn push_document(
    ctx: &Ctx,
    daemon: &mut Daemon,
    scratch: &Path,
    original: &Original,
) -> Result<(), CliError> {
    let new = match std::fs::read(scratch.join(original.doc)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(CliError::Io(e)),
    };
    if new == original.bytes {
        return Ok(());
    }
    let plan = if original.known {
        plan_mutations(&parse_file(&original.bytes), &parse_file(&new))
    } else {
        None
    };
    match plan {
        Some(mutations) if !mutations.is_empty() => {
            daemon.apply(original.doc, guarded(&original.hash, mutations))?;
            Ok(())
        }
        Some(_) => Ok(()),
        None if original.known => {
            daemon.apply(original.doc, vec![replace(&original.hash, new)])?;
            Ok(())
        }
        None => store::write(&ctx.paths.dir.join(original.doc), &parse_file(&new))
            .map_err(CliError::Store),
    }
}

/// The whole new document as one `Replace`, naming the hash `snapshot` returned for the bytes the
/// command started from: a compare-and-swap, refused (nothing written) if the document has changed.
fn replace(base_hash: &[u8], contents: Vec<u8>) -> pb::Mutation {
    let replace = pb::Replace {
        base_hash: base_hash.to_vec(),
        contents,
    };
    pb::Mutation {
        kind: Some(mutation::Kind::Replace(replace)),
    }
}

fn copy_back(scratch: &Path, dir: &Path, name: &str) -> Result<(), CliError> {
    let from = scratch.join(name);
    if !from.exists() {
        return Ok(());
    }
    let bytes = std::fs::read(&from).map_err(CliError::Io)?;
    store::write(&dir.join(name), &parse_file(&bytes)).map_err(CliError::Store)
}

pub(crate) fn line_text(line: &OwnedLine) -> Option<String> {
    line.raw().map(str::to_owned)
}

fn is_blank(line: &OwnedLine) -> bool {
    line.parse()
        .is_some_and(|l| matches!(l.kind, LineKind::Blank))
}

pub(crate) fn task_ref(old: &File, from: usize) -> pb::TaskRef {
    let task_id = old.lines[from]
        .parse()
        .and_then(|l| match l.kind {
            LineKind::Task(t) => t.id().map(|u| u.to_string()),
            LineKind::Blank => None,
        })
        .unwrap_or_default();
    let line_number = u32::try_from(from + 1).unwrap_or(u32::MAX);
    debug_assert!(line_number >= 1, "line numbers are 1-based");
    pb::TaskRef {
        line_number,
        task_id,
    }
}

/// True when every line from index `from` to the end of `new` is an insert (appends only).
fn tail_is_new(diffs: &[LineDiff], new: &File, from: usize) -> bool {
    let inserted_tail = diffs
        .iter()
        .filter(|d| matches!(d, LineDiff::Insert { to } if *to >= from))
        .count();
    inserted_tail == new.lines.len().saturating_sub(from)
}

/// The diff steps sorted into mutation kinds; positions are old-file indices.
struct Plan {
    edits: Vec<mutation::Kind>,
    deletes: Vec<(usize, bool)>,
    adds: Vec<mutation::Kind>,
    blank_inserts: Vec<usize>,
}

fn classify(diffs: &[LineDiff], old: &File, new: &File) -> Option<Plan> {
    let mut plan = Plan {
        edits: Vec::new(),
        deletes: Vec::new(),
        adds: Vec::new(),
        blank_inserts: Vec::new(),
    };
    for d in diffs {
        match d {
            LineDiff::Keep { .. } => {}
            LineDiff::Change { from, to } => {
                let new_line = line_text(&new.lines[*to])?;
                plan.edits.push(mutation::Kind::Edit(pb::Edit {
                    task: Some(task_ref(old, *from)),
                    new_line,
                }));
            }
            LineDiff::Delete { from } => {
                if is_blank(&old.lines[*from]) {
                    return None;
                }
                plan.deletes.push((*from, false));
            }
            LineDiff::Insert { to } if is_blank(&new.lines[*to]) => plan.blank_inserts.push(*to),
            LineDiff::Insert { to } => plan.adds.push(mutation::Kind::Add(pb::Add {
                line: line_text(&new.lines[*to])?,
            })),
            LineDiff::Move { .. } => return None,
        }
    }
    debug_assert!(
        plan.edits.len() + plan.deletes.len() + plan.adds.len() + plan.blank_inserts.len()
            <= diffs.len()
    );
    Some(plan)
}

/// Emits whether a diff was mutation-expressible or fell back to a whole-file write — split into
/// its own function so the tracing macro's own expansion doesn't push `plan_mutations` over the
/// cognitive-complexity budget (same pattern `client::select`'s `log_mode_selected` uses, root
/// todo.txt `logging-cli`). `result` never carries line text, only counts.
fn log_mutation_plan(result: Option<&Vec<pb::Mutation>>) {
    let (expressible, mutation_count) = match result {
        Some(mutations) => (true, mutations.len()),
        None => (false, 0),
    };
    tracing::debug!(expressible, mutation_count, "cli.mutation_plan");
}

/// The diff `old → new` as mutations: edits first (no shifts), deletes bottom-up (each shift is
/// below the next target), appends last. `None` when some step has no mutation — the CLI falls
/// back to writing the scratch bytes to the real file instead (`push_document` above).
pub fn plan_mutations(old: &File, new: &File) -> Option<Vec<pb::Mutation>> {
    let result = plan_mutations_inner(old, new).filter(|muts| reproduces(old, new, muts));
    log_mutation_plan(result.as_ref());
    result
}

/// The actual diff-to-mutations logic, split out of `plan_mutations` so `#[instrument]`-style
/// wrapping (here, the `log_mutation_plan` call) never pushes this already-branchy function over
/// the cognitive-complexity budget.
fn plan_mutations_inner(old: &File, new: &File) -> Option<Vec<pb::Mutation>> {
    if let Some(archived) = crate::archive_plan::plan(old, new) {
        return Some(archived);
    }
    let diffs = diff_lines(old, new);
    let first_task_insert = diffs
        .iter()
        .filter_map(|d| match d {
            LineDiff::Insert { to } if !is_blank(&new.lines[*to]) => Some(*to),
            _ => None,
        })
        .min();
    if let Some(f) = first_task_insert
        && !tail_is_new(&diffs, new, f)
    {
        return None;
    }
    let Plan {
        edits,
        mut deletes,
        adds,
        blank_inserts,
    } = classify(&diffs, old, new)?;
    // A delete plus an append is how a rewritten or moved line looks once lines are matched by
    // content (sidecar text has no ids, and a changed last line reads as an append). Sent that way
    // the daemon tombstones the task and mints a new one, losing its history and any concurrent
    // edit another device made to it; a whole-document `Replace` matches identity instead.
    if !deletes.is_empty() && !adds.is_empty() {
        return None;
    }
    // `del` leaves a blank in place: a task deleted at index i and a blank inserted at index i.
    for b in blank_inserts {
        let slot = deletes
            .iter_mut()
            .find(|(from, leave)| *from == b && !*leave)?;
        slot.1 = true;
    }
    deletes.sort_by_key(|(from, _)| std::cmp::Reverse(*from));
    let mut out: Vec<pb::Mutation> = edits
        .into_iter()
        .map(|k| pb::Mutation { kind: Some(k) })
        .collect();
    out.extend(deletes.into_iter().map(|(from, leave_blank)| pb::Mutation {
        kind: Some(mutation::Kind::Delete(pb::Delete {
            task: Some(task_ref(old, from)),
            leave_blank,
        })),
    }));
    out.extend(adds.into_iter().map(|k| pb::Mutation { kind: Some(k) }));
    debug_assert!(
        out.len() <= diffs.len(),
        "one mutation per diff step at most"
    );
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(old: &str, new: &str) -> Option<Vec<String>> {
        plan_mutations(&parse_file(old.as_bytes()), &parse_file(new.as_bytes())).map(|ms| {
            ms.into_iter()
                .map(|m| match m.kind {
                    Some(mutation::Kind::Add(a)) => format!("add:{}", a.line),
                    Some(mutation::Kind::Edit(e)) => {
                        format!(
                            "edit:{}:{}",
                            e.task.map(|t| t.line_number).unwrap_or(0),
                            e.new_line
                        )
                    }
                    Some(mutation::Kind::Delete(d)) => {
                        format!(
                            "del:{}:{}",
                            d.task.map(|t| t.line_number).unwrap_or(0),
                            d.leave_blank
                        )
                    }
                    other => format!("{other:?}"),
                })
                .collect()
        })
    }

    #[test]
    fn appends_edits_and_deletes_are_expressed_bottom_up() {
        assert_eq!(kinds("a\n", "a\nb\nc\n").unwrap(), vec!["add:b", "add:c"]);
        // Daemon-held lines always carry ids; an in-place edit is a same-id Change.
        let a = "a id:01ARZ3NDEKTSV4RRFFQ69G5FAA\n";
        assert_eq!(
            kinds(&format!("{a}b\n"), &format!("(A) {a}b\n")).unwrap(),
            vec![format!("edit:1:(A) {}", a.trim_end())]
        );
        assert_eq!(
            kinds("a\nb\nc\n", "b\n").unwrap(),
            vec!["del:3:false", "del:1:false"]
        );
        assert_eq!(
            kinds("a\nb\n", "\nb\n").unwrap(),
            vec!["del:1:true"],
            "todo.sh del leaves a blank"
        );
        assert_eq!(kinds("a\nb\n", "a\nb\n").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn inexpressible_diffs_fall_back() {
        assert_eq!(kinds("a\n\nb\n", "a\nb\n"), None, "blank removal (archive)");
        assert_eq!(kinds("a\nc\n", "a\nb\nc\n"), None, "mid-file insert");
        assert_eq!(
            kinds("a\n", "a\n\n"),
            None,
            "a trailing blank with no delete to pair with"
        );
    }
}
