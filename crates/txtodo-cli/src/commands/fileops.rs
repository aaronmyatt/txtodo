//! Whole-file commands: `move`, `deduplicate`, `report`. Line numbers are preserved (todo.sh
//! `TODOTXT_PRESERVE_LINE_NUMBERS=1`): a moved or duplicate line is left blank, never removed.

use crate::commands::{archive, edit::get};
use crate::{CliError, Ctx, store};
use std::collections::HashSet;
use std::path::PathBuf;
use txtodo_core::{File, OwnedLine};

/// `move ITEM# DEST [SRC]`: blank the line in SRC (default todo.txt), append it to DEST. Both are
/// names inside the todo directory and must already exist.
pub fn run_move(ctx: &Ctx, item: &str, dest: &str, src: Option<&str>) -> Result<(), CliError> {
    const USAGE: &str = "mv ITEM# DEST [SRC]";
    let src_path: PathBuf = src.map_or_else(|| ctx.paths.todo.clone(), |s| ctx.paths.dir.join(s));
    let dest_path = ctx.paths.dir.join(dest);
    if !src_path.is_file() {
        return Err(CliError::Message(format!(
            "TODO: Source file {} does not exist.",
            src_path.display()
        )));
    }
    if !dest_path.is_file() {
        return Err(CliError::Message(format!(
            "TODO: Destination file {} does not exist.",
            dest_path.display()
        )));
    }
    let mut from = store::read(&src_path)?;
    let mut to = store::read(&dest_path)?;
    let idx = get(&from, item, USAGE)?;
    let moved = String::from_utf8_lossy(from.lines[idx].bytes()).into_owned();
    let bytes = from.lines[idx].bytes().to_vec();
    from.lines[idx] = OwnedLine::from_bytes(Vec::new(), from.lines[idx].ending());
    store::append_line(&mut to, bytes);
    debug_assert!(from.lines[idx].bytes().is_empty(), "source line blanked");
    debug_assert!(
        to.lines
            .last()
            .is_some_and(|l| l.bytes() == moved.as_bytes()),
        "landed last"
    );
    store::write(&src_path, &from)?;
    store::write(&dest_path, &to)?;
    println!("{item} {moved}");
    println!(
        "TODO: {item} moved from '{}' to '{}'.",
        src_path.display(),
        dest_path.display()
    );
    Ok(())
}

/// Blanks every repeat of an earlier identical line; returns how many.
pub fn dedup(file: &mut File) -> usize {
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut removed = 0;
    for line in &mut file.lines {
        if line.bytes().is_empty() || seen.insert(line.bytes().to_vec()) {
            continue;
        }
        *line = OwnedLine::from_bytes(Vec::new(), line.ending());
        removed += 1;
    }
    debug_assert!(removed <= file.lines.len(), "bounded by the line count");
    debug_assert!(
        seen.len() + removed <= file.lines.len(),
        "every non-blank line accounted for"
    );
    removed
}

/// `deduplicate`.
pub fn run_dedup(ctx: &Ctx) -> Result<(), CliError> {
    let mut file = store::read(&ctx.paths.todo)?;
    let removed = dedup(&mut file);
    store::write(&ctx.paths.todo, &file)?;
    if removed == 0 {
        println!("TODO: No duplicate tasks found");
    } else {
        println!("TODO: {removed} duplicate task(s) removed");
    }
    Ok(())
}

/// `report`: archive, then append `TIMESTAMP TOTAL DONE` to report.txt unless the counts match the
/// last report's. `now` is `YYYY-MM-DDTHH:MM:SS` local time (todo.sh `date +%Y-%m-%dT%T`).
pub fn run_report(ctx: &Ctx, now: &str) -> Result<(), CliError> {
    archive::run(ctx)?;
    let total = store::read(&ctx.paths.todo)?.lines.len();
    let done = store::read(&ctx.paths.done)?.lines.len();
    let data = format!("{total} {done}");
    let mut report = store::read(&ctx.paths.report)?;
    let last = report
        .lines
        .last()
        .map(|l| String::from_utf8_lossy(l.bytes()).into_owned());
    let last_data = last
        .as_deref()
        .and_then(|l| l.split_once(' '))
        .map(|(_, d)| d.to_string());
    debug_assert!(now.len() == 19, "ISO timestamp without zone");
    if last_data.as_deref() == Some(data.as_str()) {
        println!("{}", last.unwrap_or_default());
        println!("TODO: Report file is up-to-date.");
        return Ok(());
    }
    let entry = format!("{now} {data}");
    store::append_line(&mut report, entry.clone().into_bytes());
    store::write(&ctx.paths.report, &report)?;
    println!("{entry}");
    println!("TODO: Report file updated.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_blanks_later_repeats_and_ignores_blank_lines() {
        let mut file = txtodo_core::parse_file(b"a\nb\na\n\n\nb\nc\n");
        assert_eq!(dedup(&mut file), 2);
        assert_eq!(file.to_bytes(), b"a\nb\n\n\n\n\nc\n");
        assert_eq!(dedup(&mut file), 0);
    }
}
