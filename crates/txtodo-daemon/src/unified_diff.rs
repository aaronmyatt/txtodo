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
    let script = edit_script(&a, &b);
    if script.iter().all(|(k, _)| *k == Kind::Same) {
        // Same lines, different bytes: a trailing newline, or CRLF against LF.
        out.push_str("\\ line endings or the final newline changed\n");
        return out;
    }
    for (start, end) in hunks(&script) {
        write_hunk(&mut out, &script, start, end);
    }
    out
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

fn write_hunk(out: &mut String, script: &[(Kind, &str)], start: usize, end: usize) {
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
    for (kind, line) in &script[start..end] {
        let sign = match kind {
            Kind::Same => ' ',
            Kind::Del => '-',
            Kind::Add => '+',
        };
        let _ = writeln!(out, "{sign}{line}");
    }
}
