//! `add` / `addm`: todo.sh `add` with `-t` (date on add).

use crate::{CliError, Ctx, store};
use txtodo_core::{Date, File, LineKind, Mode, OwnedLine, parse_line};

/// todo.sh `cleaninput` + `uppercasePriority`: CR and LF become spaces; a leading `(a)` becomes `(A)`.
pub fn clean(input: &str) -> String {
    let mut s = input.replace(['\r', '\n'], " ");
    if let [b'(', p, b')', ..] = s.as_bytes()
        && p.is_ascii_lowercase()
    {
        let upper = char::from(p.to_ascii_uppercase());
        s.replace_range(1..2, upper.encode_utf8(&mut [0; 4]));
    }
    debug_assert!(!s.contains(['\r', '\n']), "single line");
    debug_assert!(s.len() == input.len(), "length preserved");
    s
}

/// Inserts `today ` after an optional `(P) ` prefix (todo.sh `-t`), unless the line already carries a
/// creation date or is completed — todo.sh would stamp twice; the spec has one creation date.
pub fn stamp(line: &str, today: Date) -> String {
    let dated = match parse_line(line, Mode::Lenient).map(|l| l.kind) {
        Ok(LineKind::Task(t)) => t.completed || t.creation_date.is_some(),
        Ok(LineKind::Blank) | Err(_) => false,
    };
    if dated {
        return line.to_string();
    }
    let at = match line.as_bytes() {
        [b'(', p, b')', b' ', ..] if p.is_ascii_uppercase() => 4,
        _ => 0,
    };
    let out = format!("{}{today} {}", &line[..at], &line[at..]);
    debug_assert!(out.len() == line.len() + 11, "one date and one space added");
    out
}

/// One `add`: clean, stamp, append. Returns the line number and its text.
pub fn add_line(file: &mut File, input: &str, today: Date) -> (usize, String) {
    let text = stamp(&clean(input), today);
    let line = OwnedLine::from_bytes(text.into_bytes(), file.ending);
    let raw = line.raw().unwrap_or_default().to_string();
    debug_assert!(
        raw.contains(&today.to_string()) || raw.starts_with('x'),
        "dated"
    );
    let number = store::append_line(file, line.bytes().to_vec());
    debug_assert!(number == file.lines.len(), "appended at the end");
    (number, raw)
}

/// `txtodo add TEXT` / `txtodo addm TEXT` (one task per line of TEXT).
pub fn run(ctx: &Ctx, input: &str, multi: bool) -> Result<(), CliError> {
    if input.trim().is_empty() {
        return Err(CliError::Usage(if multi {
            "addm \"TODO ITEMS\""
        } else {
            "add \"TODO ITEM\""
        }));
    }
    let mut file = store::read(&ctx.paths.todo)?;
    let pieces: Vec<&str> = if multi {
        input.lines().collect()
    } else {
        vec![input]
    };
    debug_assert!(!pieces.is_empty(), "non-blank input has a line");
    for piece in pieces {
        let (number, raw) = add_line(&mut file, piece, ctx.today);
        println!("{number} {raw}");
        println!("TODO: {number} added.");
    }
    store::write(&ctx.paths.todo, &file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Date {
        Date::new(2026, 9, 11).unwrap()
    }

    #[test]
    fn clean_upcases_priority_and_flattens_line_breaks() {
        assert_eq!(clean("(a) two\r\nlines"), "(A) two  lines");
        assert_eq!(clean("(A) fine"), "(A) fine");
        assert_eq!(clean("(ab) not a priority"), "(ab) not a priority");
    }

    #[test]
    fn stamp_goes_after_the_priority_and_never_twice() {
        assert_eq!(stamp("call mum", today()), "2026-09-11 call mum");
        assert_eq!(stamp("(B) call mum", today()), "(B) 2026-09-11 call mum");
        assert_eq!(stamp("2026-01-01 old", today()), "2026-01-01 old");
        assert_eq!(stamp("x 2026-01-01 done", today()), "x 2026-01-01 done");
        assert_eq!(stamp("", today()), "2026-09-11 ");
    }

    #[test]
    fn add_line_numbers_from_the_end() {
        let mut file = txtodo_core::parse_file(b"one\n");
        let (n, raw) = add_line(&mut file, "(c) two", today());
        assert_eq!((n, raw.as_str()), (2, "(C) 2026-09-11 two"));
        assert_eq!(file.to_bytes(), b"one\n(C) 2026-09-11 two\n");
    }
}
