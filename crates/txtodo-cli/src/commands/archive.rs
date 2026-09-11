//! `archive`: todo.sh order — drop every blank line, append the `x ` lines to done.txt, delete them
//! from todo.txt. done.txt is written first so a crash in between duplicates rather than loses.

use crate::{CliError, Ctx, store};
use txtodo_core::{File, OwnedLine};

/// Removes blank lines, then moves out and returns the completed (`x `) lines, in order.
pub fn split(todo: &mut File) -> Vec<OwnedLine> {
    let before = todo.lines.len();
    todo.lines.retain(|l| !l.bytes().is_empty());
    let (done, kept): (Vec<OwnedLine>, Vec<OwnedLine>) = todo
        .lines
        .drain(..)
        .partition(|l| l.bytes().starts_with(b"x "));
    todo.lines = kept;
    todo.trailing_newline = todo
        .lines
        .last()
        .is_none_or(|l| l.ending() != txtodo_core::LineEnding::None);
    debug_assert!(done.len() + todo.lines.len() <= before, "lines only leave");
    debug_assert!(
        todo.lines.iter().all(|l| !l.bytes().starts_with(b"x ")),
        "no done lines remain"
    );
    done
}

/// `txtodo archive`.
pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let mut todo = store::read(&ctx.paths.todo)?;
    let mut done = store::read(&ctx.paths.done)?;
    let moved = split(&mut todo);
    if moved.is_empty() {
        store::write(&ctx.paths.todo, &todo)?;
        println!(
            "TODO: {} does not contain any done tasks.",
            ctx.paths.todo.display()
        );
        return Ok(());
    }
    for line in &moved {
        println!("{}", String::from_utf8_lossy(line.bytes()));
        store::append_line(&mut done, line.bytes().to_vec());
    }
    debug_assert!(done.lines.len() >= moved.len(), "every moved line landed");
    store::write(&ctx.paths.done, &done)?;
    store::write(&ctx.paths.todo, &todo)?;
    println!("TODO: {} archived.", ctx.paths.todo.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_drops_blanks_and_pulls_done_lines_in_order() {
        let mut todo =
            txtodo_core::parse_file(b"x 2026-09-11 a\n\nkeep\nx 2026-09-11 b\nxylophone\n");
        let moved: Vec<&[u8]> = split(&mut todo)
            .iter()
            .map(|l| l.bytes().to_vec())
            .collect::<Vec<_>>()
            .leak()
            .iter()
            .map(Vec::as_slice)
            .collect();
        assert_eq!(moved, [b"x 2026-09-11 a".as_slice(), b"x 2026-09-11 b"]);
        assert_eq!(todo.to_bytes(), b"keep\nxylophone\n");
        let mut all_done = txtodo_core::parse_file(b"x 2026-09-11 a");
        split(&mut all_done);
        assert_eq!((all_done.lines.len(), all_done.trailing_newline), (0, true));
    }
}
