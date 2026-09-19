//! `fmt` and `lint`: the one place quirks are canonicalised (design §2.2 rule 7 — explicit, never a
//! side effect) and the report of what a file carries.

use crate::{CliError, Ctx, json, store};
use txtodo_core::{Edit, File, LineKind, OwnedLine, Prefix, Quirks, apply, emit_prefix};

/// The strict spelling of a line: canonical prefix, single spaces, no trailing whitespace, a
/// completed line's priority moved to `pri:`. `None` when the line is already canonical or opaque.
pub fn canonical(line: &OwnedLine) -> Option<Vec<u8>> {
    let parsed = line.parse()?;
    let LineKind::Task(task) = parsed.kind else {
        return None;
    };
    let mut prefix = Prefix::of(&task);
    let moved = if task.completed {
        prefix.priority.take()
    } else {
        None
    };
    let words: Vec<&str> = task
        .description
        .split([' ', '\t'])
        .filter(|w| !w.is_empty())
        .collect();
    let description = words.join(" ");
    let text = format!(
        "{}{description}",
        emit_prefix(&prefix, !description.is_empty())
    );
    let mut out = OwnedLine::from_bytes(text.into_bytes(), line.ending());
    if let Some(p) = moved {
        let edit = Edit::new()
            .set_tag("pri", p.as_char().encode_utf8(&mut [0; 4]))
            .ok()?;
        out = apply(&out, &edit);
    }
    debug_assert!(!out.bytes().ends_with(b" "), "no trailing whitespace");
    debug_assert!(
        out.parse()
            .is_some_and(|l| !l.quirks.has(Quirks::TRAILING_WS)),
        "trailing_ws gone"
    );
    (out.bytes() != line.bytes()).then(|| out.bytes().to_vec())
}

/// Rewrites every line canonically, unifies endings on the dominant one, ensures a trailing newline.
/// Returns how many lines changed.
pub fn format(file: &mut File) -> usize {
    let ending = file.ending;
    let mut changed = 0;
    for line in &mut file.lines {
        let bytes = canonical(line).unwrap_or_else(|| line.bytes().to_vec());
        if bytes != line.bytes() || line.ending() != ending {
            changed += 1;
        }
        *line = OwnedLine::from_bytes(bytes, ending);
    }
    file.trailing_newline = !file.lines.is_empty();
    debug_assert!(changed <= file.lines.len(), "bounded");
    debug_assert!(
        file.lines.iter().all(|l| l.ending() == ending),
        "one ending"
    );
    changed
}

/// `txtodo fmt`.
pub fn run_fmt(ctx: &Ctx) -> Result<(), CliError> {
    let mut file = store::read(&ctx.paths.todo)?;
    let before = file.to_bytes();
    let changed = format(&mut file);
    if file.to_bytes() != before {
        store::write(&ctx.paths.todo, &file)?;
    }
    println!("TODO: {changed} line(s) reformatted.");
    Ok(())
}

/// root todo 9: "add line length hints to the clients... to encourage keeping todo entries
/// readable" — 100 matches the line-width budget this project's own Rust code is held to
/// (`.claude/budgets.json`'s `lineWidth`), not a todo.txt-format rule; purely advisory, `lint`
/// only reports it, nothing rejects or rewrites a longer line.
const LINE_LENGTH_HINT: usize = 100;

/// `Some` past [`LINE_LENGTH_HINT`], `None` otherwise.
fn length_hint(line: &OwnedLine) -> Option<String> {
    let len = line.bytes().len();
    (len > LINE_LENGTH_HINT).then(|| format!("{len} chars, over the {LINE_LENGTH_HINT}-char hint"))
}

/// Every finding for one line (1-based `number`): the parse/quirks check plus the length hint.
/// Split out of `findings` to keep that function's cognitive-complexity budget — a loop body
/// this branchy counts against the *caller*, not just the callee, so the whole per-line shape
/// has to move, not just the new check.
fn line_findings(number: usize, line: &OwnedLine) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    match line.parse() {
        None => out.push((number, "not valid UTF-8".to_string())),
        Some(l) if !l.quirks.is_empty() => out.push((number, l.quirks.to_string())),
        Some(_) => {}
    }
    if let Some(finding) = length_hint(line) {
        out.push((number, finding));
    }
    out
}

/// Per-line findings: `(number, description)`, then file-level ones with number 0.
pub fn findings(file: &File) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (i, line) in file.lines.iter().enumerate() {
        out.extend(line_findings(i + 1, line));
    }
    if file.bom {
        out.push((0, "byte order mark".to_string()));
    }
    if !file.trailing_newline && !file.lines.is_empty() {
        out.push((0, "no trailing newline".to_string()));
    }
    debug_assert!(
        out.iter().all(|(n, _)| *n <= file.lines.len()),
        "numbers within the file"
    );
    debug_assert!(
        out.iter().all(|(_, d)| !d.is_empty()),
        "every finding says something"
    );
    out
}

/// `txtodo lint`: report only, exit 0.
pub fn run_lint(ctx: &Ctx) -> Result<(), CliError> {
    let file = store::read(&ctx.paths.todo)?;
    let found = findings(&file);
    if ctx.json {
        for (n, d) in &found {
            println!(r#"{{"line":{n},"finding":{}}}"#, json::str(d));
        }
        return Ok(());
    }
    for (n, d) in &found {
        if *n == 0 {
            println!("file: {d}")
        } else {
            println!("{n}: {d}")
        }
    }
    println!("TODO: {} finding(s).", found.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_core::LineEnding;

    #[test]
    fn canonical_fixes_every_lenient_corpus_quirk_and_leaves_strict_lines_alone() {
        let file = txtodo_core::parse_file(include_bytes!("../../../../corpus/lenient.txt"));
        let got: Vec<Option<String>> = file
            .lines
            .iter()
            .map(|l| canonical(l).map(|b| String::from_utf8(b).unwrap()))
            .collect();
        assert_eq!(got[0], None, "x without a date has no strict spelling");
        assert_eq!(
            got[1].as_deref(),
            Some("x 2026-09-11 priority after x pri:A")
        );
        assert_eq!(
            got[2].as_deref(),
            Some("x 2026-09-11 priority after date pri:A")
        );
        assert_eq!(got[3].as_deref(), Some("2026-09-11 tab separated words"));
        assert_eq!(got[4].as_deref(), Some("2026-09-11 trailing whitespace"));
        assert_eq!((got[5].as_deref(), got[6].as_deref()), (None, None));
        assert_eq!(got[7].as_deref(), Some("2026-09-11 runs of spaces"));
        let plain = OwnedLine::from_bytes(
            b"(A) 2026-09-11 fine +p @c due:2026-09-12".to_vec(),
            LineEnding::Lf,
        );
        assert_eq!(canonical(&plain), None);
    }

    #[test]
    fn format_unifies_endings_and_findings_cover_file_hygiene() {
        let mut file = txtodo_core::parse_file(b"\xEF\xBB\xBFa \r\nb\r\nc\n\xFF");
        let expect = [
            (1, "trailing_ws"),
            (3, "mixed_ending"),
            (4, "not valid UTF-8"),
            (0, "byte order mark"),
            (0, "no trailing newline"),
        ];
        assert_eq!(findings(&file), expect.map(|(n, d)| (n, d.to_string())));
        assert_eq!(
            format(&mut file),
            3,
            "line 1 text, line 3 ending, line 4 ending"
        );
        assert_eq!(file.to_bytes(), b"\xEF\xBB\xBFa\r\nb\r\nc\r\n\xFF\r\n");
    }

    /// root todo 9: a line past the 100-char hint is reported, a line at or under it is not.
    #[test]
    fn findings_reports_lines_over_the_length_hint_only() {
        let exactly_100 = "a".repeat(100);
        let over_100 = "a".repeat(101);
        let text = format!("{exactly_100}\n{over_100}\n");
        let file = txtodo_core::parse_file(text.as_bytes());
        assert_eq!(
            findings(&file),
            vec![(2, "101 chars, over the 100-char hint".to_string())]
        );
    }
}
