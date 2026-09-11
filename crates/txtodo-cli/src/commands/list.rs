//! Listings: todo.sh `_format` — number every line, drop blanks, keep lines matching every term,
//! sort like `env LC_COLLATE=C sort -f -k2`, print zero-padded numbers, then the `N of M` footer.

use crate::{CliError, Ctx, store};
use std::path::{Path, PathBuf};
use txtodo_core::{File, LineKind, Mode, parse_line};

/// One listed line: its 1-based number and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Line number in the file (0 for done.txt lines under `listall`, as todo.sh prints them).
    pub number: usize,
    /// The line without its ending.
    pub raw: String,
}

/// todo.sh `filtercommand`: each term is a case-insensitive substring; a leading `-` excludes it.
pub fn matches(raw: &str, terms: &[String]) -> bool {
    let hay = raw.to_lowercase();
    let ok = terms.iter().all(|t| match t.strip_prefix('-') {
        Some(neg) if !neg.is_empty() => !hay.contains(&neg.to_lowercase()),
        _ => hay.contains(&t.to_lowercase()),
    });
    debug_assert!(!terms.is_empty() || ok, "no terms keeps every line");
    ok
}

/// Sort key of `sort -f -k2` under `LC_COLLATE=C`: ASCII-uppercased text, ties by line number.
fn sort_key(item: &Item) -> (Vec<u8>, usize) {
    (
        item.raw.bytes().map(|b| b.to_ascii_uppercase()).collect(),
        item.number,
    )
}

/// Numbered non-blank lines matching `terms`, sorted. Opaque (non-UTF-8) lines list lossily.
pub fn items(file: &File, terms: &[String]) -> Vec<Item> {
    let mut out: Vec<Item> = file
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| Item {
            number: i + 1,
            raw: String::from_utf8_lossy(l.bytes()).into_owned(),
        })
        .filter(|it| !it.raw.trim_matches([' ', '\t']).is_empty() && matches(&it.raw, terms))
        .collect();
    out.sort_by_cached_key(sort_key);
    debug_assert!(out.len() <= file.lines.len(), "a filter never adds lines");
    debug_assert!(
        out.windows(2).all(|w| sort_key(&w[0]) <= sort_key(&w[1])),
        "sorted"
    );
    out
}

/// Digits in the line count (todo.sh `getPadding`); at least 1.
fn padding(total: usize) -> usize {
    total.max(1).to_string().len()
}

/// todo.sh `getPrefix`: the file's base name without extension, uppercased.
pub fn prefix(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_uppercase())
        .unwrap_or_default()
}

/// Plain: `NN raw` per item. `--json`: one object per item, footers omitted.
fn print_items(ctx: &Ctx, items: &[Item], width: usize) {
    for it in items {
        if ctx.json {
            println!("{}", crate::json::item(it));
        } else {
            println!("{:0width$} {}", it.number, it.raw);
        }
    }
}

fn separator(ctx: &Ctx) {
    if !ctx.json {
        println!("--");
    }
}

fn footer(ctx: &Ctx, path: &Path, shown: usize, total: usize) {
    if ctx.json {
        return;
    }
    println!("{}: {shown} of {total} tasks shown", prefix(path));
}

/// `list` / `listfile FILE`: one file, filtered, with its footer.
pub fn list_file(ctx: &Ctx, path: &Path, terms: &[String]) -> Result<(), CliError> {
    let file = store::read(path)?;
    let shown = items(&file, terms);
    print_items(ctx, &shown, padding(file.lines.len()));
    separator(ctx);
    footer(ctx, path, shown.len(), file.lines.len());
    Ok(())
}

/// `listpri [P|P-Q] [TERMS]`: only lines whose priority falls in the range (default A-Z).
pub fn list_pri(ctx: &Ctx, args: &[String]) -> Result<(), CliError> {
    let (lo, hi, terms) = match args.first().map(|a| a.to_ascii_uppercase()) {
        Some(p) if matches!(p.as_bytes(), [b'A'..=b'Z']) => {
            (p.as_bytes()[0], p.as_bytes()[0], &args[1..])
        }
        Some(p) if matches!(p.as_bytes(), [b'A'..=b'Z', b'-', b'A'..=b'Z']) => {
            (p.as_bytes()[0], p.as_bytes()[2], &args[1..])
        }
        _ => (b'A', b'Z', args),
    };
    debug_assert!(
        lo.is_ascii_uppercase() && hi.is_ascii_uppercase(),
        "letters"
    );
    let file = store::read(&ctx.paths.todo)?;
    let shown: Vec<Item> = items(&file, terms)
        .into_iter()
        .filter(
            |it| matches!(it.raw.as_bytes(), [b'(', p, b')', b' ', ..] if (lo..=hi).contains(p)),
        )
        .collect();
    print_items(ctx, &shown, padding(file.lines.len()));
    separator(ctx);
    footer(ctx, &ctx.paths.todo, shown.len(), file.lines.len());
    Ok(())
}

/// `listall [TERMS]`: todo.txt then done.txt; done lines are numbered 0 like todo.sh's awk step.
pub fn list_all(ctx: &Ctx, terms: &[String]) -> Result<(), CliError> {
    let todo = store::read(&ctx.paths.todo)?;
    let done = store::read(&ctx.paths.done)?;
    let width = padding(todo.lines.len());
    let shown = items(&todo, terms);
    let shown_done: Vec<Item> = items(&done, terms)
        .into_iter()
        .map(|it| Item {
            number: 0,
            raw: it.raw,
        })
        .collect();
    print_items(ctx, &shown, width);
    print_items(ctx, &shown_done, width);
    separator(ctx);
    footer(ctx, &ctx.paths.todo, shown.len(), todo.lines.len());
    footer(ctx, &ctx.paths.done, shown_done.len(), done.lines.len());
    let (n, m) = (
        shown.len() + shown_done.len(),
        todo.lines.len() + done.lines.len(),
    );
    debug_assert!(n <= m, "shown within total");
    if !ctx.json {
        println!("total {n} of {m} tasks shown");
    }
    Ok(())
}

/// `listproj` (`+`) / `listcon` (`@`): the unique projects or contexts of matching lines, sorted.
pub fn list_words(ctx: &Ctx, sigil: char, terms: &[String]) -> Result<(), CliError> {
    let file = store::read(&ctx.paths.todo)?;
    let mut words: Vec<String> = Vec::new();
    for it in items(&file, terms) {
        let Ok(line) = parse_line(&it.raw, Mode::Lenient) else {
            continue;
        };
        let LineKind::Task(task) = line.kind else {
            continue;
        };
        let found: Vec<&str> = if sigil == '+' {
            task.projects().collect()
        } else {
            task.contexts().collect()
        };
        words.extend(found.into_iter().map(|w| format!("{sigil}{w}")));
    }
    words.sort();
    words.dedup();
    debug_assert!(words.iter().all(|w| w.starts_with(sigil)), "sigil kept");
    if ctx.json {
        println!("{}", crate::json::strs(words.iter().map(String::as_str)));
        return Ok(());
    }
    for w in words {
        println!("{w}");
    }
    Ok(())
}

/// todo.sh `_list` lookup: absolute, `<dir>/FILE`, `./FILE`, then `<dir>/FILE.txt`.
pub fn find_file(ctx: &Ctx, name: &str) -> Result<PathBuf, CliError> {
    let given = PathBuf::from(name);
    let candidates = [
        given.clone(),
        ctx.paths.dir.join(name),
        ctx.paths.dir.join(format!("{name}.txt")),
    ];
    debug_assert!(candidates.len() == 3, "three fallbacks");
    if given.is_absolute() && given.is_file() {
        return Ok(given);
    }
    candidates
        .into_iter()
        .find(|c| c.is_file())
        .ok_or_else(|| CliError::Message(format!("TODO: File {name} does not exist.")))
}

/// `listfile` with no name: the `*.txt` files in the todo directory, sorted.
pub fn list_txt_files(ctx: &Ctx) -> Result<(), CliError> {
    let mut names: Vec<String> = std::fs::read_dir(&ctx.paths.dir)?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".txt"))
        .collect();
    names.sort();
    debug_assert!(names.iter().all(|n| n.ends_with(".txt")), "txt only");
    if ctx.json {
        println!("{}", crate::json::strs(names.iter().map(String::as_str)));
        return Ok(());
    }
    println!("Files in the todo.txt directory:");
    for n in names {
        println!("{n}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_filter_case_insensitively_with_negation() {
        let terms = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(matches("Call Mum @phone", &terms(&["mum", "@PHONE"])));
        assert!(!matches("Call Mum @phone", &terms(&["mum", "-phone"])));
        assert!(matches("anything", &terms(&[])));
    }

    #[test]
    fn items_drop_blanks_number_from_one_and_sort_case_folded() {
        let file = txtodo_core::parse_file(b"beta\n\n(A) alpha\nAlpha\n\t\n");
        let listed = items(&file, &[]);
        let got: Vec<(usize, &str)> = listed.iter().map(|i| (i.number, i.raw.as_str())).collect();
        assert_eq!(got, [(3, "(A) alpha"), (4, "Alpha"), (1, "beta")]);
        assert_eq!((padding(9), padding(10), padding(0)), (1, 2, 1));
        assert_eq!(prefix(Path::new("/x/done.txt")), "DONE");
    }
}
