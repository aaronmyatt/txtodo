//! Single-line mutations with todo.sh semantics: `do`, `pri`, `depri`. `do` goes through
//! `core::Edit::complete` (the `pri:` rule); the rest are the same raw-text edits todo.sh makes.

use crate::commands::archive;
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

/// `do ITEM#...`: complete via the core (priority becomes `pri:P`), then archive unless disabled.
pub fn run_do(ctx: &Ctx, args: &[String]) -> Result<(), CliError> {
    const USAGE: &str = "do ITEM#[, ITEM#, ITEM#, ...]";
    let items = split_items(args);
    if items.is_empty() {
        return Err(CliError::Usage(USAGE));
    }
    let mut file = store::read(&ctx.paths.todo)?;
    for item in &items {
        let idx = get(&file, item, USAGE)?;
        if file.lines[idx].bytes().starts_with(b"x ") {
            println!("TODO: {item} is already marked done.");
            continue;
        }
        file.lines[idx] = apply(&file.lines[idx], &Edit::new().complete(ctx.today));
        debug_assert!(file.lines[idx].bytes().starts_with(b"x "), "completed");
        println!("{item} {}", raw_of(&file.lines[idx]));
        println!("TODO: {item} marked as done.");
    }
    store::write(&ctx.paths.todo, &file)?;
    if ctx.auto_archive {
        archive::run(ctx)
    } else {
        Ok(())
    }
}

/// `pri ITEM# PRIORITY`: todo.sh strips any `(X) ` prefix and prepends the new one.
pub fn run_pri(ctx: &Ctx, item: &str, priority: &str) -> Result<(), CliError> {
    const USAGE: &str = "pri ITEM# PRIORITY\nnote: PRIORITY must be anywhere from A to Z.";
    let new = match priority.as_bytes() {
        [p] if p.is_ascii_alphabetic() => char::from(p.to_ascii_uppercase()),
        _ => return Err(CliError::Usage(USAGE)),
    };
    let mut file = store::read(&ctx.paths.todo)?;
    let idx = get(&file, item, USAGE)?;
    let raw = raw_of(&file.lines[idx]);
    let old = priority_prefix(&raw);
    if old == Some(new) {
        println!("{item} {raw}");
        println!("TODO: {item} already prioritized ({new}).");
        return Ok(());
    }
    let rest = if old.is_some() {
        &raw[4..]
    } else {
        raw.as_str()
    };
    let text = format!("({new}) {rest}");
    debug_assert!(priority_prefix(&text) == Some(new), "new prefix in place");
    set_raw(&mut file, idx, &text);
    store::write(&ctx.paths.todo, &file)?;
    println!("{item} {text}");
    match old {
        Some(o) => println!("TODO: {item} re-prioritized from ({o}) to ({new})."),
        None => println!("TODO: {item} prioritized ({new})."),
    }
    Ok(())
}

/// `depri ITEM#...`: drop a `(X) ` prefix.
pub fn run_depri(ctx: &Ctx, args: &[String]) -> Result<(), CliError> {
    const USAGE: &str = "depri ITEM#[, ITEM#, ITEM#, ...]";
    let items = split_items(args);
    if items.is_empty() {
        return Err(CliError::Usage(USAGE));
    }
    let mut file = store::read(&ctx.paths.todo)?;
    for item in &items {
        let idx = get(&file, item, USAGE)?;
        let raw = raw_of(&file.lines[idx]);
        if priority_prefix(&raw).is_none() {
            println!("TODO: {item} is not prioritized.");
            continue;
        }
        set_raw(&mut file, idx, &raw[4..]);
        debug_assert!(!file.lines[idx].bytes().starts_with(b"("), "prefix gone");
        println!("{item} {}", &raw[4..]);
        println!("TODO: {item} deprioritized.");
    }
    Ok(store::write(&ctx.paths.todo, &file)?)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
