//! `append`, `prepend`, `replace`: todo.sh's raw-text edits. The priority and creation-date prefix
//! survives `prepend`/`replace` exactly as todo.sh's `replaceOrPrepend` keeps it.

use crate::commands::add::clean;
use crate::commands::edit::get;
use crate::{CliError, Ctx, store};
use txtodo_core::{File, OwnedLine};

/// todo.sh `SENTENCE_DELIMITERS`: appended text starting with one of these gets no leading space.
const SENTENCE_DELIMITERS: [char; 4] = [',', '.', ':', ';'];

/// Length of todo.sh's `^\((.) \)\{0,1\}\([0-9]\{2,4\}-[0-9]\{2\}-[0-9]\{2\} \)\{0,1\}` match on `raw`:
/// `(pri_len, date_len)`, each 0 when absent.
pub fn prefix_lens(raw: &str) -> (usize, usize) {
    let pri = match raw.chars().take(4).collect::<Vec<_>>().as_slice() {
        ['(', p, ')', ' '] => 3 + p.len_utf8(),
        _ => 0,
    };
    let date = date_len(&raw[pri..]);
    debug_assert!(pri + date <= raw.len(), "prefix within the line");
    (pri, date)
}

/// `[0-9]{2,4}-[0-9]{2}-[0-9]{2} ` at the start, greedy on the year digits; 0 when absent.
fn date_len(s: &str) -> usize {
    let b = s.as_bytes();
    let year = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if !(2..=4).contains(&year) {
        return 0;
    }
    let rest = &b[year..];
    let shape = rest.len() >= 7
        && rest[0] == b'-'
        && rest[1..3].iter().all(u8::is_ascii_digit)
        && rest[3] == b'-'
        && rest[4..6].iter().all(u8::is_ascii_digit)
        && rest[6] == b' ';
    if shape { year + 7 } else { 0 }
}

/// `raw` + (space unless `text` opens with a sentence delimiter) + `text`.
pub fn append(raw: &str, text: &str) -> String {
    let sep = if text.starts_with(SENTENCE_DELIMITERS) {
        ""
    } else {
        " "
    };
    let out = format!("{raw}{sep}{text}");
    debug_assert!(out.starts_with(raw) && out.ends_with(text), "both kept");
    out
}

/// Prefix kept, then `text`, a space, and the rest of the line.
pub fn prepend(raw: &str, text: &str) -> String {
    let (p, d) = prefix_lens(raw);
    let out = format!("{}{text} {}", &raw[..p + d], &raw[p + d..]);
    debug_assert!(out.len() == raw.len() + text.len() + 1, "one space added");
    out
}

/// Old prefix kept, except that a priority or date at the start of `text` replaces the old one
/// and is then stripped from `text` (todo.sh 2.14 `replaceOrPrepend`).
pub fn replace(raw: &str, text: &str) -> String {
    let (p, d) = prefix_lens(raw);
    let (np, nd) = prefix_lens(text);
    let pri = if np > 0 { &text[..np] } else { &raw[..p] };
    let date = if nd > 0 {
        &text[np..np + nd]
    } else {
        &raw[p..p + d]
    };
    let out = format!("{pri}{date}{}", &text[np + nd..]);
    debug_assert!(out.ends_with(&text[np + nd..]), "body is the tail");
    debug_assert!(prefix_lens(&out).0 == pri.len(), "one priority");
    out
}

/// Which raw edit a command performs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `append`.
    Append,
    /// `prepend`.
    Prepend,
    /// `replace`.
    Replace,
}

/// `txtodo append|prepend|replace ITEM# TEXT`.
pub fn run(ctx: &Ctx, kind: Kind, item: &str, text: &str) -> Result<(), CliError> {
    let usage = match kind {
        Kind::Append => "append ITEM# \"TEXT TO APPEND\"",
        Kind::Prepend => "prepend ITEM# \"TEXT TO PREPEND\"",
        Kind::Replace => "replace ITEM# \"UPDATED ITEM\"",
    };
    if text.is_empty() {
        return Err(CliError::Usage(usage));
    }
    let mut file: File = store::read(&ctx.paths.todo)?;
    let idx = get(&file, item, usage)?;
    let old = String::from_utf8_lossy(file.lines[idx].bytes()).into_owned();
    let text = clean(text);
    let new = match kind {
        Kind::Append => append(&old, &text),
        Kind::Prepend => prepend(&old, &text),
        Kind::Replace => replace(&old, &text),
    };
    debug_assert!(new.contains(text.as_str()), "the text landed");
    file.lines[idx] = OwnedLine::from_bytes(new.clone().into_bytes(), file.lines[idx].ending());
    store::write(&ctx.paths.todo, &file)?;
    if kind == Kind::Replace {
        println!("{item} {old}");
        println!("TODO: Replaced task with:");
    }
    println!("{item} {new}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_lens_follow_the_todo_sh_regex() {
        assert_eq!(prefix_lens("(A) 2026-09-11 x"), (4, 11));
        assert_eq!(prefix_lens("26-09-11 x"), (0, 9));
        assert_eq!(prefix_lens("(A) x"), (4, 0));
        assert_eq!(prefix_lens("2026-09-11x"), (0, 0));
        assert_eq!(prefix_lens("x 2026-09-11 done"), (0, 0));
    }

    #[test]
    fn append_prepend_replace_keep_what_todo_sh_keeps() {
        assert_eq!(append("a", "b"), "a b");
        assert_eq!(append("a", ", b"), "a, b");
        assert_eq!(
            prepend("(A) 2026-09-11 rest", "new"),
            "(A) 2026-09-11 new rest"
        );
        assert_eq!(prepend("plain", "new"), "new plain");
        assert_eq!(replace("(A) 2026-09-11 old", "new"), "(A) 2026-09-11 new");
        assert_eq!(
            replace("(A) 2026-09-11 old", "2026-01-01 new"),
            "(A) 2026-01-01 new"
        );
        assert_eq!(replace("2026-09-11 old", "(B) new"), "(B) 2026-09-11 new");
        assert_eq!(
            replace("(A) 2026-09-11 old", "(B) new"),
            "(B) 2026-09-11 new"
        );
    }
}
