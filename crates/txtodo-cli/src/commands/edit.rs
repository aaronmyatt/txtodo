//! Single-line mutations with todo.sh semantics: `do`, `pri`, `depri`, `del`. `do` goes through
//! `core::Edit::complete` (the `pri:` rule); the rest are the same raw-text edits todo.sh makes.

use crate::{CliError, Ctx, store};
use txtodo_core::{Edit, File, OwnedLine, apply};

/// todo.sh `getTodo`: a decimal line number naming a non-blank line. Returns the 0-based index.
pub fn get(file: &File, item: &str, usage: &'static str) -> Result<usize, CliError> {
    if item.is_empty() || !item.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CliError::Usage(usage));
    }
    let n: usize = item.parse().map_err(|_| CliError::Usage(usage))?;
    let idx = n.checked_sub(1).filter(|&i| i < file.lines.len());
    match idx {
        Some(i) if !file.lines[i].bytes().is_empty() => Ok(i),
        _ => Err(CliError::Message(format!("TODO: No task {item}."))),
    }
}

/// `ITEM#[, ITEM#, ...]`: todo.sh splits on commas and whitespace.
pub fn split_items(args: &[String]) -> Vec<String> {
    let items: Vec<String> = args
        .iter()
        .flat_map(|a| a.split([',', ' ']))
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    debug_assert!(items.iter().all(|i| !i.contains(',')), "no commas remain");
    items
}

fn raw_of(line: &OwnedLine) -> String {
    String::from_utf8_lossy(line.bytes()).into_owned()
}

/// Replaces line `idx` with `text`, keeping its ending.
fn set_raw(file: &mut File, idx: usize, text: &str) {
    let ending = file.lines[idx].ending();
    file.lines[idx] = OwnedLine::from_bytes(text.as_bytes().to_vec(), ending);
    debug_assert!(file.lines[idx].ending() == ending, "ending kept");
}

/// `(X) ` at the start of a line, any single character X (todo.sh `^(.) `).
fn priority_prefix(raw: &str) -> Option<char> {
    let mut chars = raw.chars();
    match (chars.next(), chars.next(), chars.next(), chars.next()) {
        (Some('('), Some(p), Some(')'), Some(' ')) => Some(p),
        _ => None,
    }
}

/// Emits the already-done diagnostic — split into its own function so the tracing macro's own
/// expansion doesn't push `run_do` over the cognitive-complexity budget (same pattern
/// `txtodo-telemetry`'s `emit_info_event`/`emit_warn_event` tests use). `item` is the user's
/// `ITEM#` argument (a line number), never task text.
fn log_already_done(item: &str) {
    tracing::warn!(item, "cli.already_done");
}

/// Moves the lines `do` just completed (`indices`, any order) to the end of the file, in their
/// old relative order (task complete-to-bottom). Only those lines: a done line a human put
/// somewhere else stays there, and blank lines stay too, which the old `archive` after every `do`
/// did not respect. `txtodo archive` is still the explicit "sort every done line, drop blanks".
fn move_to_end(file: &mut txtodo_core::File, mut indices: Vec<usize>) {
    indices.sort_unstable();
    indices.dedup();
    let before = file.lines.len();
    let mut moved = Vec::with_capacity(indices.len());
    // Back to front, so each removal leaves the indices still to come untouched.
    for idx in indices.into_iter().rev() {
        moved.push(file.lines.remove(idx));
    }
    for line in moved.into_iter().rev() {
        store::append_line(file, line.bytes().to_vec());
    }
    debug_assert_eq!(file.lines.len(), before, "lines only move");
}

/// `do ITEM#...`: complete via the core (priority becomes `pri:P`), then move the completed lines
/// to the end of the file unless `-A` says to leave them where they are.
/// An already-done item is reported (todo.sh 2.14 prints it to stderr; here it becomes a tracing
/// event, see root todo.txt `logging-cli`) and fails the run.
pub fn run_do(ctx: &Ctx, args: &[String]) -> Result<(), CliError> {
    const USAGE: &str = "do ITEM#[, ITEM#, ITEM#, ...]";
    let items = split_items(args);
    if items.is_empty() {
        return Err(CliError::Usage(USAGE));
    }
    let mut file = store::read(&ctx.paths.todo)?;
    let mut failed = false;
    let mut completed = Vec::new();
    for item in &items {
        let idx = get(&file, item, USAGE)?;
        if file.lines[idx].bytes().starts_with(b"x ") {
            log_already_done(item);
            failed = true;
            continue;
        }
        file.lines[idx] = apply(&file.lines[idx], &Edit::new().complete(ctx.today));
        debug_assert!(file.lines[idx].bytes().starts_with(b"x "), "completed");
        println!("{item} {}", raw_of(&file.lines[idx]));
        println!("TODO: {item} marked as done.");
        completed.push(idx);
    }
    if ctx.auto_archive {
        move_to_end(&mut file, completed);
    }
    store::write(&ctx.paths.todo, &file)?;
    if failed {
        Err(CliError::Reported)
    } else {
        Ok(())
    }
}

/// Emits the already-prioritized diagnostic — split out for the same cognitive-complexity reason
/// as `log_already_done` above. `item` is the user's `ITEM#` argument, `priority` the single
/// priority letter; neither is task text.
fn log_already_prioritized(item: &str, priority: char) {
    tracing::warn!(item, %priority, "cli.already_prioritized");
}

/// `pri ITEM# PRIORITY [ITEM# PRIORITY ...]`: todo.sh strips any `(X) ` prefix and prepends the new
/// one. An item already at that priority is reported (a tracing event now, see `log_already_done`'s
/// doc) and fails the run.
pub fn run_pri(ctx: &Ctx, args: &[String]) -> Result<(), CliError> {
    const USAGE: &str =
        "pri ITEM# PRIORITY [ITEM# PRIORITY ...]\nnote: PRIORITY must be anywhere from A to Z.";
    if args.is_empty() || !args.len().is_multiple_of(2) {
        return Err(CliError::Usage(USAGE));
    }
    let mut file = store::read(&ctx.paths.todo)?;
    let mut failed = false;
    for pair in args.chunks_exact(2) {
        let (item, priority) = (&pair[0], &pair[1]);
        let new = match priority.as_bytes() {
            [p] if p.is_ascii_alphabetic() => char::from(p.to_ascii_uppercase()),
            _ => return Err(CliError::Usage(USAGE)),
        };
        let idx = get(&file, item, USAGE)?;
        let raw = raw_of(&file.lines[idx]);
        let old = priority_prefix(&raw);
        if old == Some(new) {
            println!("{item} {raw}");
            log_already_prioritized(item, new);
            failed = true;
            continue;
        }
        let rest = if old.is_some() {
            &raw[4..]
        } else {
            raw.as_str()
        };
        let text = format!("({new}) {rest}");
        debug_assert!(priority_prefix(&text) == Some(new), "new prefix in place");
        set_raw(&mut file, idx, &text);
        println!("{item} {text}");
        match old {
            Some(o) => println!("TODO: {item} re-prioritized from ({o}) to ({new})."),
            None => println!("TODO: {item} prioritized ({new})."),
        }
    }
    store::write(&ctx.paths.todo, &file)?;
    if failed {
        Err(CliError::Reported)
    } else {
        Ok(())
    }
}

/// Emits the not-prioritized diagnostic — split out for the same cognitive-complexity reason as
/// `log_already_done` above.
fn log_not_prioritized(item: &str) {
    tracing::warn!(item, "cli.not_prioritized");
}

/// `depri ITEM#...`: drop a `(X) ` prefix; an unprioritised item is reported (a tracing event now,
/// see `log_already_done`'s doc) and fails the run.
pub fn run_depri(ctx: &Ctx, args: &[String]) -> Result<(), CliError> {
    const USAGE: &str = "depri ITEM#[, ITEM#, ITEM#, ...]";
    let items = split_items(args);
    if items.is_empty() {
        return Err(CliError::Usage(USAGE));
    }
    let mut file = store::read(&ctx.paths.todo)?;
    let mut failed = false;
    for item in &items {
        let idx = get(&file, item, USAGE)?;
        let raw = raw_of(&file.lines[idx]);
        if priority_prefix(&raw).is_none() {
            log_not_prioritized(item);
            failed = true;
            continue;
        }
        set_raw(&mut file, idx, &raw[4..]);
        debug_assert!(!file.lines[idx].bytes().starts_with(b"("), "prefix gone");
        println!("{item} {}", &raw[4..]);
        println!("TODO: {item} deprioritized.");
    }
    store::write(&ctx.paths.todo, &file)?;
    if failed {
        Err(CliError::Reported)
    } else {
        Ok(())
    }
}

/// `del ITEM# [TERM]`: blank the line (line numbers are preserved), or remove TERM from it.
pub fn run_del(ctx: &Ctx, item: &str, term: Option<&str>) -> Result<(), CliError> {
    const USAGE: &str = "del ITEM# [TERM]";
    let mut file = store::read(&ctx.paths.todo)?;
    let idx = get(&file, item, USAGE)?;
    let raw = raw_of(&file.lines[idx]);
    let Some(term) = term else {
        set_raw(&mut file, idx, "");
        store::write(&ctx.paths.todo, &file)?;
        println!("{item} {raw}");
        println!("TODO: {item} deleted.");
        return Ok(());
    };
    let new = remove_term(&raw, term);
    if new == raw {
        println!("{item} {raw}");
        return Err(CliError::Message(format!(
            "TODO: '{term}' not found; no removal done."
        )));
    }
    set_raw(&mut file, idx, &new);
    store::write(&ctx.paths.todo, &file)?;
    println!("{item} {raw}");
    println!("TODO: Removed '{term}' from task.");
    println!("{item} {new}");
    Ok(())
}

/// todo.sh `del ITEM TERM`, its five sed substitutions in order, TERM taken literally:
/// `^\((.) \)\{0,1\} *T *` → prefix · ` *T *$` → `` · `  *T *` → ` ` · ` *T  *` → ` ` · `T` → ``.
pub fn remove_term(raw: &str, term: &str) -> String {
    let mut s = raw.to_string();
    let at = if priority_prefix(&s).is_some() { 4 } else { 0 };
    let body = s[at..].trim_start_matches(' ');
    if let Some(rest) = body.strip_prefix(term) {
        s = format!("{}{}", &s[..at], rest.trim_start_matches(' '));
    }
    let trimmed = s.trim_end_matches(' ');
    if let Some(head) = trimmed.strip_suffix(term) {
        s = head.trim_end_matches(' ').to_string();
    }
    s = replace_spaced(&s, term, 1, 0);
    s = replace_spaced(&s, term, 0, 1);
    let out = s.replace(term, "");
    debug_assert!(out.len() <= raw.len(), "removal never grows the line");
    debug_assert!(
        term.is_empty() || !out.contains(term),
        "every occurrence gone"
    );
    out
}

/// `s/ {min_before,}T {min_after,}/ /g` with spaces greedy on both sides (sed leftmost-longest).
fn replace_spaced(s: &str, term: &str, min_before: usize, min_after: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let before = s[i..].len() - s[i..].trim_start_matches(' ').len();
        let after_term = i + before + term.len();
        let hit = before >= min_before && s[i + before..].starts_with(term) && {
            let after = s[after_term..].len() - s[after_term..].trim_start_matches(' ').len();
            after >= min_after && {
                out.push(' ');
                i = after_term + after;
                true
            }
        };
        if !hit {
            let c = s[i..].chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
    }
    debug_assert!(i == s.len(), "consumed the whole line");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_term_matches_todo_sh_at_start_end_and_middle() {
        assert_eq!(remove_term("(A) foo bar baz", "foo"), "(A) bar baz");
        assert_eq!(remove_term("(A) foo bar baz", "baz"), "(A) foo bar");
        assert_eq!(remove_term("(A) foo bar baz", "bar"), "(A) foo baz");
        assert_eq!(remove_term("a due:x b", "due:x"), "a b");
        assert_eq!(remove_term("a bxb b", "x"), "a bb b");
        assert_eq!(remove_term("nothing here", "zzz"), "nothing here");
    }

    fn err(r: Result<usize, CliError>) -> String {
        r.map(|i| i.to_string()).unwrap_or_else(|e| e.to_string())
    }

    #[test]
    fn get_rejects_non_numbers_blanks_and_out_of_range() {
        let file = txtodo_core::parse_file(
            b"one

three
",
        );
        assert_eq!(err(get(&file, "3", "u")), "2");
        assert_eq!(err(get(&file, "2", "u")), "TODO: No task 2.");
        assert_eq!(err(get(&file, "0", "u")), "TODO: No task 0.");
        assert_eq!(err(get(&file, "x", "u")), "usage: txtodo u");
        assert_eq!(split_items(&["1,2".into(), "3".into()]), ["1", "2", "3"]);
        assert_eq!(
            (priority_prefix("(a) x"), priority_prefix("(A)x")),
            (Some('a'), None)
        );
    }

    /// Task complete-to-bottom: `do` moves the lines it completed, and only those. The done line a
    /// human left in the middle and the blank line both stay where they are.
    #[test]
    fn move_to_end_moves_only_the_named_lines_in_their_old_order() {
        let mut file = txtodo_core::parse_file(b"x c\nx old done\n\nopen\nx a\n");
        move_to_end(&mut file, vec![4, 0]);
        assert_eq!(file.to_bytes(), b"x old done\n\nopen\nx c\nx a\n");
        let mut last_unterminated = txtodo_core::parse_file(b"x a\nopen");
        move_to_end(&mut last_unterminated, vec![0]);
        assert_eq!(last_unterminated.to_bytes(), b"open\nx a\n");
        let mut none = txtodo_core::parse_file(b"a\nb\n");
        move_to_end(&mut none, Vec::new());
        assert_eq!(none.to_bytes(), b"a\nb\n");
    }
}
