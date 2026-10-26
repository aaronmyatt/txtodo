//! `move`/`mv` through the daemon, and `copy`/`cp` (task add-the-ability-to-move-or-copy-a-task-t).
//! In daemon mode `mv` used to run in the scratch copy, which holds only the root list, so it
//! always failed ("Destination file … does not exist"); it now sends the daemon's own cross-file
//! `Move`, which keeps the task's history and moves its `ref:` folder beside the new list. A copy
//! is a new task: the same line at the end of the other list, minus `id:` and `ref:` (its notes
//! and sub-list stay with the original), dated today when it carries no creation date.

use crate::client::Daemon;
use crate::commands::{edit::get, list};
use crate::{CliError, Ctx, store};
use std::path::Path;
use txtodo_core::{Edit, OwnedLine, parse_file};
use txtodo_proto::v1::{self as pb, mutation::Kind};

const MOVE_USAGE: &str = "mv ITEM# DEST [SRC]";
const COPY_USAGE: &str = "cp ITEM# DEST [SRC]";

/// The line a copy adds: `raw` without its `id:` and `ref:` tags. A line the core cannot edit
/// (opaque bytes) is copied as it is.
pub fn copy_text(raw: &str) -> String {
    let edit = Edit::new()
        .remove_tag("id")
        .and_then(|e| e.remove_tag("ref"));
    let Ok(edit) = edit else {
        return raw.to_owned();
    };
    let line = OwnedLine::from_bytes(raw.as_bytes().to_vec(), txtodo_core::LineEnding::default());
    let out = txtodo_core::apply(&line, &edit);
    out.raw().unwrap_or(raw).to_owned()
}

/// `copy ITEM# DEST [SRC]`, direct mode: append the copy to DEST; SRC is untouched.
pub fn run_copy(ctx: &Ctx, item: &str, dest: &str, src: Option<&str>) -> Result<(), CliError> {
    let src_path = src.map_or_else(|| ctx.paths.todo.clone(), |s| ctx.paths.dir.join(s));
    let dest_path = ctx.paths.dir.join(dest);
    exists(&src_path, "Source")?;
    exists(&dest_path, "Destination")?;
    let from = store::read(&src_path)?;
    let idx = get(&from, item, COPY_USAGE)?;
    let raw = String::from_utf8_lossy(from.lines[idx].bytes()).into_owned();
    // A new task: dated today when it has no creation date, as `add` and the daemon's `Add` do.
    let copied = txtodo_core::line_fixes::stamp_created(&copy_text(&raw), ctx.today);
    let mut to = store::read(&dest_path)?;
    let number = store::append_line(&mut to, copied.clone().into_bytes());
    store::write(&dest_path, &to)?;
    report(item, &copied, (&src_path, &dest_path), ("copied", number));
    Ok(())
}

/// `move ITEM# DEST [SRC]` in daemon mode: one cross-file `Move` (its `ref:` folder goes too).
pub fn move_via_daemon(
    ctx: &Ctx,
    daemon: &mut Daemon,
    item: &str,
    dest: &str,
    src: Option<&str>,
) -> Result<(), CliError> {
    let (src_doc, dest_doc) = documents(ctx, daemon, dest, src)?;
    let (bytes, ids) = daemon.task_ids(&src_doc)?;
    let idx = get(&parse_file(&bytes), item, MOVE_USAGE)?;
    let raw = line_text(&bytes, idx);
    let task = pb::TaskRef {
        line_number: u32::try_from(idx + 1).unwrap_or(u32::MAX),
        task_id: ids.get(idx).cloned().unwrap_or_default(),
    };
    let to_path = dest_doc.clone();
    let kind = Kind::Move(pb::Move {
        task: Some(task),
        to_path,
    });
    daemon.apply(&src_doc, vec![pb::Mutation { kind: Some(kind) }])?;
    let number = landed_at(daemon, &dest_doc, &raw)?;
    let paths = (ctx.paths.dir.join(&src_doc), ctx.paths.dir.join(&dest_doc));
    report(item, &raw, (&paths.0, &paths.1), ("moved", number));
    Ok(())
}

/// `copy ITEM# DEST [SRC]` in daemon mode: an `Add` of the copy to DEST.
pub fn copy_via_daemon(
    ctx: &Ctx,
    daemon: &mut Daemon,
    item: &str,
    dest: &str,
    src: Option<&str>,
) -> Result<(), CliError> {
    let (src_doc, dest_doc) = documents(ctx, daemon, dest, src)?;
    let (bytes, _) = daemon.snapshot(&src_doc)?;
    let idx = get(&parse_file(&bytes), item, COPY_USAGE)?;
    let copied = copy_text(&line_text(&bytes, idx));
    let kind = Kind::Add(pb::Add {
        line: copied.clone(),
    });
    daemon.apply(&dest_doc, vec![pb::Mutation { kind: Some(kind) }])?;
    let number = landed_at(daemon, &dest_doc, &copied)?;
    let paths = (ctx.paths.dir.join(&src_doc), ctx.paths.dir.join(&dest_doc));
    report(item, &copied, (&paths.0, &paths.1), ("copied", number));
    Ok(())
}

/// The source (SRC, else this list) and destination as the daemon names them, both refused like
/// todo.sh's when the daemon holds no such list.
fn documents(
    ctx: &Ctx,
    daemon: &mut Daemon,
    dest: &str,
    src: Option<&str>,
) -> Result<(String, String), CliError> {
    let listed: Vec<String> = daemon.list_files()?.into_iter().map(|f| f.path).collect();
    let src_doc = src.map_or_else(|| ctx.paths.todo_file.clone(), str::to_owned);
    for (doc, which) in [(src_doc.as_str(), "Source"), (dest, "Destination")] {
        if !listed.iter().any(|l| l == doc) {
            return Err(missing(&ctx.paths.dir.join(doc), which));
        }
    }
    Ok((src_doc, dest.to_owned()))
}

fn exists(path: &Path, which: &str) -> Result<(), CliError> {
    match path.is_file() {
        true => Ok(()),
        false => Err(missing(path, which)),
    }
}

fn missing(path: &Path, which: &str) -> CliError {
    let shown = path.display();
    CliError::Message(format!("TODO: {which} file {shown} does not exist."))
}

fn line_text(bytes: &[u8], idx: usize) -> String {
    let file = parse_file(bytes);
    String::from_utf8_lossy(file.lines[idx].bytes()).into_owned()
}

/// The line number `text` landed at in `doc`: the last line that is exactly it, else the last
/// line with text (a `ref:` slug the move had to rename no longer matches).
fn landed_at(daemon: &mut Daemon, doc: &str, text: &str) -> Result<usize, CliError> {
    let (bytes, _) = daemon.snapshot(doc)?;
    let file = parse_file(&bytes);
    let lines: Vec<&[u8]> = file.lines.iter().map(OwnedLine::bytes).collect();
    let exact = lines.iter().rposition(|l| *l == text.as_bytes());
    let last = || lines.iter().rposition(|l| !l.is_empty());
    Ok(exact.or_else(last).map_or(0, |i| i + 1))
}

/// todo.sh's two lines: the task, then where it went.
fn report(item: &str, text: &str, (src, dest): (&Path, &Path), (verb, number): (&str, usize)) {
    println!("{item} {text}");
    let (src_name, dest_name) = (list::prefix(src), list::prefix(dest));
    println!("{src_name}: {item} {verb} to {number} in {dest_name}.");
}

#[cfg(test)]
mod tests {
    use super::copy_text;

    #[test]
    fn a_copy_drops_the_id_and_the_ref_and_keeps_everything_else() {
        assert_eq!(
            copy_text(
                "(A) 2026-10-17 plan trip +home ref:trip id:01ARZ3NDEKTSV4RRFFQ69G5FAV due:2026-11-01"
            ),
            "(A) 2026-10-17 plan trip +home due:2026-11-01"
        );
        assert_eq!(copy_text("buy milk @shop"), "buy milk @shop");
        assert_eq!(copy_text("x 2026-10-17 done ref:a"), "x 2026-10-17 done");
    }
}
