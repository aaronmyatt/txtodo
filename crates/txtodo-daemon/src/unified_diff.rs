//! A unified diff of two documents (task apply-dry-run), so a dry-run `Apply` can show what a batch
//! would change. Line based, three lines of context, `--- a/<path>` and `+++ b/<path>` headers:
//! the format `git diff` and `patch` read.
//! Ref: https://www.gnu.org/software/diffutils/manual/html_node/Unified-Format.html
//!
//! No dependency: the workspace manifest is frozen, and a todo file's edits are small. The common
//! prefix and suffix are trimmed first, then a longest-common-subsequence table runs over what is
//! left; past `MAX_LCS_CELLS` the middle is shown as one whole replacement, which is still a
//! correct diff, only a coarser one.

use std::fmt::Write;

/// Lines of unchanged text kept around each change.
const CONTEXT: usize = 3;
/// Most cells of the LCS table (4 bytes each): a 2 000 x 2 000 middle. Bounds time and memory.
const MAX_LCS_CELLS: usize = 4_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Same,
    Del,
    Add,
}

/// The diff from `old` to `new` for the document at `path`; empty when they are the same bytes.
pub(crate) fn unified_diff(path: &str, old: &[u8], new: &[u8]) -> String {
    if old == new {
        return String::new();
    }
    let (old, new) = (String::from_utf8_lossy(old), String::from_utf8_lossy(new));
    let (a, b): (Vec<&str>, Vec<&str>) = (
        old.split_terminator('\n').collect(),
        new.split_terminator('\n').collect(),
    );
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    let mut script = edit_script(&a, &b);
    let ends = Ends {
        old_lines: a.len(),
        new_lines: b.len(),
        old_newline: old.ends_with('\n'),
        new_newline: new.ends_with('\n'),
    };
    // Only the final newline (or a CRLF/LF change inside the last line) differs: the last line
    // is shown as removed and added, so the `\ No newline at end of file` marker below has a
    // side to attach to — what `git diff` prints for the same change.
    if script.iter().all(|(k, _)| *k == Kind::Same)
        && let Some((_, last)) = script.pop()
    {
        script.push((Kind::Del, last));
        script.push((Kind::Add, last));
    }
    for (start, end) in hunks(&script) {
        write_hunk(&mut out, &script, start, end, &ends);
    }
    out
}

/// Where each document ends, for the `\ No newline at end of file` marker (the unified format's
/// own token: https://www.gnu.org/software/diffutils/manual/html_node/Incomplete-Lines.html).
struct Ends {
    old_lines: usize,
    new_lines: usize,
    old_newline: bool,
    new_newline: bool,
}

/// True when the line just written is the last of its document and that document has no
/// trailing newline — the marker goes on the next line.
fn incomplete(ends: &Ends, old_idx: Option<usize>, new_idx: Option<usize>) -> bool {
    let old_last = old_idx.is_some_and(|i| i + 1 == ends.old_lines) && !ends.old_newline;
    let new_last = new_idx.is_some_and(|i| i + 1 == ends.new_lines) && !ends.new_newline;
    old_last || new_last
}

/// One entry per line of the merged document, in order.
fn edit_script<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(Kind, &'a str)> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut script: Vec<(Kind, &str)> = a[..prefix].iter().map(|l| (Kind::Same, *l)).collect();
    script.extend(middle(am, bm));
    script.extend(a[a.len() - suffix..].iter().map(|l| (Kind::Same, *l)));
    script
}

/// The edit script of the differing middle: an LCS walk, or a whole replacement when too big.
fn middle<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(Kind, &'a str)> {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_LCS_CELLS {
        return a
            .iter()
            .map(|l| (Kind::Del, *l))
            .chain(b.iter().map(|l| (Kind::Add, *l)))
            .collect();
    }
    // lcs[i][j]: length of the longest common subsequence of a[i..] and b[j..].
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if a[i] == b[j] {
                lcs[at(i + 1, j + 1)] + 1
            } else {
                lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(n + m);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push((Kind::Same, a[i]));
            i += 1;
            j += 1;
        } else if j == m || (i < n && lcs[at(i + 1, j)] >= lcs[at(i, j + 1)]) {
            out.push((Kind::Del, a[i]));
            i += 1;
        } else {
            out.push((Kind::Add, b[j]));
            j += 1;
        }
    }
    out
}

/// `[start, end)` ranges of the script, each holding changes with `CONTEXT` lines around them;
/// ranges whose context would touch are merged.
fn hunks(script: &[(Kind, &str)]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, (kind, _)) in script.iter().enumerate() {
        if *kind == Kind::Same {
            continue;
        }
        let (start, end) = (
            i.saturating_sub(CONTEXT),
            (i + 1 + CONTEXT).min(script.len()),
        );
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => out.push((start, end)),
        }
    }
    out
}

fn write_hunk(out: &mut String, script: &[(Kind, &str)], start: usize, end: usize, ends: &Ends) {
    let count = |range: &[(Kind, &str)], keep: Kind| {
        range
            .iter()
            .filter(|(k, _)| *k == Kind::Same || *k == keep)
            .count()
    };
    let (old_before, new_before) = (
        count(&script[..start], Kind::Del),
        count(&script[..start], Kind::Add),
    );
    let (old_len, new_len) = (
        count(&script[start..end], Kind::Del),
        count(&script[start..end], Kind::Add),
    );
    // An empty side is numbered by the line before it (`-0,0` at the top of a file).
    let first = |before: usize, len: usize| if len == 0 { before } else { before + 1 };
    let _ = writeln!(
        out,
        "@@ -{},{old_len} +{},{new_len} @@",
        first(old_before, old_len),
        first(new_before, new_len)
    );
    let (mut old_idx, mut new_idx) = (old_before, new_before);
    for (kind, line) in &script[start..end] {
        let (sign, old_at, new_at) = match kind {
            Kind::Same => (' ', Some(old_idx), Some(new_idx)),
            Kind::Del => ('-', Some(old_idx), None),
            Kind::Add => ('+', None, Some(new_idx)),
        };
        old_idx += usize::from(old_at.is_some());
        new_idx += usize::from(new_at.is_some());
        let _ = writeln!(out, "{sign}{line}");
        if incomplete(ends, old_at, new_at) {
            out.push_str("\\ No newline at end of file\n");
        }
    }
}

#[cfg(test)]
mod newline_tests {
    use super::unified_diff;

    #[test]
    fn a_missing_final_newline_gets_the_standard_marker_not_prose() {
        let diff = unified_diff("todo.txt", b"a\nb\n", b"a\nb");
        assert!(
            diff.contains("-b\n+b\n\\ No newline at end of file\n"),
            "{diff}"
        );
        assert!(!diff.contains("line endings"), "{diff}");
        assert!(
            diff.starts_with("--- a/todo.txt\n+++ b/todo.txt\n@@ -1,2 +1,2 @@\n"),
            "{diff}"
        );
    }

    #[test]
    fn a_change_on_an_incomplete_last_line_marks_both_sides() {
        let diff = unified_diff("todo.txt", b"a\nold", b"a\nnew");
        assert!(
            diff.contains("-old\n\\ No newline at end of file\n"),
            "{diff}"
        );
        assert!(
            diff.contains("+new\n\\ No newline at end of file\n"),
            "{diff}"
        );
    }

    #[test]
    fn complete_documents_never_get_the_marker() {
        let diff = unified_diff("todo.txt", b"a\nb\n", b"a\nc\n");
        assert!(!diff.contains("No newline"), "{diff}");
        assert!(diff.contains("-b\n+c\n"), "{diff}");
    }
}
