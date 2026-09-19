//! What `txtodo lint` reports about a file, moved here out of `txtodo-cli` so the daemon can run
//! the identical check for a client that may not link this crate (the MCP server's `todo_lint`,
//! root todo id:01M2T868JD99Y6FV3XKETKZAZH, through the `Lint` RPC). Read-only: nothing here
//! rewrites a line — canonicalising quirks is `txtodo fmt`'s job.

use crate::{File, LINE_LENGTH_HINT, OwnedLine, over_length_hint};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// `Some` when the line is past the advisory length hint. A non-UTF-8 line is already reported as
/// such and has no character length.
fn length_hint(line: &OwnedLine) -> Option<String> {
    let len = over_length_hint(line.raw()?)?;
    Some(format!(
        "{len} chars, over the {LINE_LENGTH_HINT}-char hint"
    ))
}

/// Every finding for one line (1-based `number`): the parse/quirks check plus the length hint.
/// Split out of [`findings`] to keep that function's cognitive-complexity budget — a loop body
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_file;

    /// root todo 9: a line past the 100-char hint is reported, a line at or under it is not.
    #[test]
    fn findings_reports_lines_over_the_length_hint_only() {
        let exactly_100 = "a".repeat(100);
        let over_100 = "a".repeat(101);
        let text = format!("{exactly_100}\n{over_100}\n");
        let file = parse_file(text.as_bytes());
        assert_eq!(
            findings(&file),
            alloc::vec![(2, "101 chars, over the 100-char hint".to_string())]
        );
    }

    #[test]
    fn findings_counts_chars_and_not_the_lines_own_id_tag() {
        // 60 CJK chars are 180 bytes: only a char count leaves them under the hint.
        let cjk = "字".repeat(60);
        // 75 visible chars plus a 30-char id tag and its blank: 105 raw, 75 as a human sees it.
        let tagged = format!("{} id:01J9K3H5Z7Q8X2M4N6P8R0T2V4", "a".repeat(75));
        let file = parse_file(format!("{cjk}\n{tagged}\n").as_bytes());
        assert_eq!(findings(&file), Vec::<(usize, String)>::new());
    }

    #[test]
    fn a_file_reports_its_bom_and_missing_trailing_newline_with_line_zero() {
        let file = parse_file(b"\xEF\xBB\xBFa\nb");
        assert_eq!(
            findings(&file),
            alloc::vec![
                (0, "byte order mark".to_string()),
                (0, "no trailing newline".to_string())
            ]
        );
    }

    #[test]
    fn a_non_utf8_line_is_reported_and_has_no_length() {
        let file = parse_file(b"ok\n\xFF\n");
        assert_eq!(
            findings(&file),
            alloc::vec![(2, "not valid UTF-8".to_string())]
        );
    }
}
