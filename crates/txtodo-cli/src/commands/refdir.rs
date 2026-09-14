//! `open`, `notes`, `sub`, `prune --orphans` (plan M5, specs/ref-directories.md rules 2, 3, 4, 10,
//! 12): thin daemon-mode wrappers over the `RefDir`/`GetNotes`/`EditNotes`/`PruneOrphans` RPCs —
//! never a direct file write, matching every other daemon-only command in this crate.

use crate::client::Daemon;
use crate::{CliError, Ctx, json};
use txtodo_proto::v1 as pb;

/// A decimal `ITEM#` (1-based, blanks included) into the wire `TaskRef` shape every RPC in this
/// file takes; the daemon resolves the actual line and its id (`RefDirRequest`/`GetNotes` ignore
/// or fill in `task_id` themselves, so this crate never parses `id:` tags — sidecar mode carries
/// none in the text at all).
fn task_ref(item: &str, usage: &'static str) -> Result<pb::TaskRef, CliError> {
    let line_number: u32 = item.parse().map_err(|_| CliError::Usage(usage))?;
    if line_number == 0 {
        return Err(CliError::Usage(usage));
    }
    Ok(pb::TaskRef {
        line_number,
        task_id: String::new(),
    })
}

/// `open ITEM#`: prints the resolved (or would-be) `ref:` directory; never creates anything —
/// `ensure = false` on the `RefDir` call is the whole negative-space guarantee.
pub fn run_open(ctx: &Ctx, daemon: &mut Daemon, item: &str) -> Result<(), CliError> {
    let task = task_ref(item, "open ITEM#")?;
    let info = daemon.ref_dir("todo.txt", task, false)?;
    println!("{}", ctx.paths.dir.join(&info.dir).display());
    Ok(())
}

/// `notes ITEM#`: lazily creates the `ref:` directory (rule 4, one op batch, before `$EDITOR`
/// opens per the M5 acceptance wording), then opens `$EDITOR` on its `notes.md`.
pub fn run_notes(daemon: &mut Daemon, item: &str) -> Result<(), CliError> {
    let task = task_ref(item, "notes ITEM#")?;
    let info = daemon.ref_dir("todo.txt", task, true)?;
    let bare = pb::TaskRef {
        line_number: 0,
        task_id: info.task_id,
    };
    let doc = daemon.get_notes(bare.clone())?;
    let new_text = edit_in_editor(&doc.bytes)?;
    if new_text.as_bytes() == doc.bytes {
        return Ok(());
    }
    daemon.edit_notes(pb::NotesEditRequest {
        task: Some(bare),
        new_text,
        workspace: None,
    })?;
    Ok(())
}

/// Writes `current` to a scratch `notes.md`, runs `$EDITOR` on it and reads the result back. A
/// missing `$EDITOR` is a clear error, not a panic (cli-ref-commands/notes.md).
fn edit_in_editor(current: &[u8]) -> Result<String, CliError> {
    let editor = std::env::var("EDITOR")
        .map_err(|_| CliError::Message("txtodo: notes needs $EDITOR set".to_owned()))?;
    let dir = tempfile::tempdir().map_err(CliError::Io)?;
    let scratch = dir.path().join("notes.md");
    std::fs::write(&scratch, current).map_err(CliError::Io)?;
    let status = std::process::Command::new(&editor)
        .arg(&scratch)
        .status()
        .map_err(CliError::Io)?;
    if !status.success() {
        return Err(CliError::Message(format!(
            "txtodo: {editor} exited with {status}"
        )));
    }
    let bytes = std::fs::read(&scratch).map_err(CliError::Io)?;
    String::from_utf8(bytes).map_err(|_| CliError::Message("txtodo: notes.md must be UTF-8".into()))
}

/// `sub ITEM# COMMAND...`: re-execs this same binary with `--dir` scoped to the line's `ref:`
/// sub-list (rule 12) — a scoped `todo.sh -d <ref>/todo.cfg`. Requires an existing `ref:` tag
/// (typed error otherwise); a dangling one (rule 9) is healed first, same as the first sub-list
/// write would, since the tag already exists and `ensure` can mint no new one.
pub fn run_sub(ctx: &Ctx, daemon: &mut Daemon, item: &str, cmd: &[String]) -> Result<(), CliError> {
    let task = task_ref(item, "sub ITEM# COMMAND...")?;
    let probe = daemon.ref_dir("todo.txt", task.clone(), false)?;
    if !probe.has_ref_tag {
        return Err(CliError::Message(format!(
            "txtodo: line {item} has no ref: tag; run `txtodo notes {item}` first"
        )));
    }
    let info = daemon.ref_dir("todo.txt", task, true)?;
    let dir = ctx.paths.dir.join(&info.dir);
    let exe = std::env::current_exe().map_err(CliError::Io)?;
    let status = std::process::Command::new(exe)
        .arg("--dir")
        .arg(&dir)
        .args(cmd)
        .status()
        .map_err(CliError::Io)?;
    if status.success() {
        Ok(())
    } else {
        // The child already printed its own error; only the exit status carries over.
        Err(CliError::Reported)
    }
}

/// `prune --orphans [--yes]`: lists directories no line points to (rule 10); deletes them only
/// with `--yes`.
pub fn run_prune(
    daemon: &mut Daemon,
    orphans: bool,
    yes: bool,
    as_json: bool,
) -> Result<(), CliError> {
    if !orphans {
        return Err(CliError::Usage("prune --orphans [--yes]"));
    }
    let resp = daemon.prune_orphans(yes)?;
    if as_json {
        println!("{}", json::strs(resp.dirs.iter().map(String::as_str)));
        return Ok(());
    }
    if resp.dirs.is_empty() {
        println!("TODO: no orphaned ref: directories.");
        return Ok(());
    }
    for d in &resp.dirs {
        println!("{d}");
    }
    if yes {
        println!("TODO: deleted {} orphaned director(ies).", resp.dirs.len());
    } else {
        println!("TODO: run with --yes to delete.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_ref_rejects_non_numbers_and_zero() {
        assert_eq!(task_ref("3", "u").unwrap().line_number, 3);
        assert!(task_ref("0", "u").is_err());
        assert!(task_ref("x", "u").is_err());
        assert!(task_ref("3", "u").unwrap().task_id.is_empty());
    }
}
