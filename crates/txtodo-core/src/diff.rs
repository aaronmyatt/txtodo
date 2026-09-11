//! Line-level and character-level diffs. `diff_lines` keys lines by `id:` when both sides have one, else by
//! content, so the reconciler (M3) can turn an external edit into ops. `diff_text` is char-level for the
//! CRDT's text type (M4). Both use Myers' greedy forward algorithm, bounded, without recursion.
//! Ref: E. Myers, "An O(ND) Difference Algorithm and Its Variations", 1986.

use crate::{File, LineKind, Mode, OwnedLine, Ulid};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// One step of a line diff. Indices are line positions in `a` (`from`) and `b` (`to`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineDiff {
    /// Same line on both sides.
    Keep {
        /// Index in `a`.
        from: usize,
        /// Index in `b`.
        to: usize,
    },
    /// Only in `b`.
    Insert {
        /// Index in `b`.
        to: usize,
    },
    /// Only in `a`.
    Delete {
        /// Index in `a`.
        from: usize,
    },
    /// Same `id:` on both sides, different bytes.
    Change {
        /// Index in `a`.
        from: usize,
        /// Index in `b`.
        to: usize,
    },
    /// Same `id:` deleted at `from` and inserted at `to`.
    Move {
        /// Index in `a`.
        from: usize,
        /// Index in `b`.
        to: usize,
    },
}

/// One step of a text diff, at Unicode scalar (char) granularity. Positions index the original text and
/// the edits apply in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextEdit {
    /// Insert `text` before char index `at`.
    Insert {
        /// Char index in the original.
        at: usize,
        /// Chars to insert.
        text: String,
    },
    /// Delete `len` chars starting at char index `at`.
    Delete {
        /// Char index in the original.
        at: usize,
        /// Number of chars.
        len: usize,
    },
}

/// How a line is matched across files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Id(Ulid),
    Content(u64),
}

/// FNV-1a over the line bytes: cheap, `no_std`, and only ever compared within one diff.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn key_of(line: &OwnedLine) -> Key {
    let id = line.raw().and_then(|r| crate::parse_line(r, Mode::Lenient).ok()).and_then(|l| match l.kind {
        LineKind::Task(t) => t.id(),
        LineKind::Blank => None,
    });
    id.map_or_else(|| Key::Content(fnv1a(line.bytes())), Key::Id)
}

/// Diffs two files line by line. Lines with a valid `id:` match by id (a changed line is `Change`, a
/// relocated one `Move`); all others match by exact bytes. Blank lines match by bytes (empty).
pub fn diff_lines(a: &File, b: &File) -> Vec<LineDiff> {
    let ka: Vec<Key> = a.lines.iter().map(key_of).collect();
    let kb: Vec<Key> = b.lines.iter().map(key_of).collect();
    let mut out: Vec<LineDiff> = myers(&ka, &kb)
        .into_iter()
        .map(|s| match s {
            Step::Keep(i, j) if a.lines[i].bytes() == b.lines[j].bytes() => LineDiff::Keep { from: i, to: j },
            Step::Keep(i, j) => LineDiff::Change { from: i, to: j },
            Step::Delete(i) => LineDiff::Delete { from: i },
            Step::Insert(j) => LineDiff::Insert { to: j },
        })
        .collect();
    pair_moves(&mut out, &ka, &kb);
    debug_assert!(out.iter().filter(|d| matches!(d, LineDiff::Keep { .. } | LineDiff::Change { .. } | LineDiff::Move { .. } | LineDiff::Delete { .. })).count() <= a.lines.len() + b.lines.len());
    out
}

/// Turns a `Delete` + `Insert` of the same `Id` key into one `Move`.
fn pair_moves(out: &mut Vec<LineDiff>, ka: &[Key], kb: &[Key]) {
    let mut i = 0;
    while i < out.len() {
        let LineDiff::Delete { from } = out[i] else {
            i += 1;
            continue;
        };
        let Key::Id(id) = ka[from] else {
            i += 1;
            continue;
        };
        let inserted = out.iter().position(|d| matches!(d, LineDiff::Insert { to } if kb[*to] == Key::Id(id)));
        match inserted {
            Some(j) => {
                let LineDiff::Insert { to } = out[j] else { unreachable!("matched Insert above") };
                out[i] = LineDiff::Move { from, to };
                out.remove(j);
            }
            None => i += 1,
        }
    }
}

/// Char-level diff of `a` → `b`, merged into runs.
pub fn diff_text(a: &str, b: &str) -> Vec<TextEdit> {
    let ca: Vec<char> = a.chars().collect();
    let cb: Vec<char> = b.chars().collect();
    let mut out: Vec<TextEdit> = Vec::new();
    for step in myers(&ca, &cb) {
        match (step, out.last_mut()) {
            (Step::Delete(i), Some(TextEdit::Delete { at, len })) if *at + *len == i => *len += 1,
            (Step::Delete(i), _) => out.push(TextEdit::Delete { at: i, len: 1 }),
            (Step::Insert(j), Some(TextEdit::Insert { at, text })) if *at + text.chars().count() == j => text.push(cb[j]),
            (Step::Insert(j), _) => out.push(TextEdit::Insert { at: j, text: String::from(cb[j]) }),
            (Step::Keep(..), _) => {}
        }
    }
    out
}

/// A raw diff step over indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Keep(usize, usize),
    Delete(usize),
    Insert(usize),
}

/// Greedy forward Myers: O((N+M)·D) time and space, D ≤ N+M. Returns steps in order. No recursion.
fn myers<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Step> {
    let (n, m) = (a.len(), b.len());
    let max = n + m;
    let off = max;
    let mut v: Vec<usize> = vec![0; 2 * max + 2];
    let mut trace: Vec<Vec<usize>> = Vec::new();
    for d in 0..=max {
        debug_assert!(d <= max, "the outer loop is bounded by N+M");
        trace.push(v.clone());
        let mut k = -(d as isize);
        while k <= d as isize {
            let ki = (k + off as isize) as usize;
            let down = k == -(d as isize) || (k != d as isize && v[ki - 1] < v[ki + 1]);
            let mut x = if down { v[ki + 1] } else { v[ki - 1] + 1 };
            let mut y = (x as isize - k) as usize;
            while x < n && y < m && a[x] == b[y] {
                x += 1;
                y += 1;
            }
            v[ki] = x;
            if x >= n && y >= m {
                return backtrack(&trace, a.len(), b.len(), off);
            }
            k += 2;
        }
    }
    unreachable!("a path always exists within D = N+M")
}

/// Walks the recorded V arrays back from (n, m) to (0, 0) and reverses the steps. Signed arithmetic:
/// the previous diagonal of `k = -d` sits at `y = -1`, which is fine to compare against and never indexed.
fn backtrack(trace: &[Vec<usize>], n: usize, m: usize, off: usize) -> Vec<Step> {
    let (mut x, mut y) = (n as isize, m as isize);
    let mut steps = Vec::new();
    for (d, v) in trace.iter().enumerate().rev() {
        let d = d as isize;
        let k = x - y;
        let at = |kk: isize| v[(kk + off as isize) as usize] as isize;
        let down = k == -d || (k != d && at(k - 1) < at(k + 1));
        let prev_k = if down { k + 1 } else { k - 1 };
        let (prev_x, prev_y) = if d == 0 { (0, 0) } else { (at(prev_k), at(prev_k) - prev_k) };
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            steps.push(Step::Keep(x as usize, y as usize));
        }
        if d > 0 {
            steps.push(if down { Step::Insert(prev_y as usize) } else { Step::Delete(prev_x as usize) });
            x = prev_x;
            y = prev_y;
        }
    }
    debug_assert!(x == 0 && y == 0, "backtrack reaches the origin");
    steps.reverse();
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_file;

    fn f(s: &str) -> File {
        parse_file(s.as_bytes())
    }
    const A: &str = "one id:01J9K3H5Z7Q8X2M4N6P8R0T2V4";
    const B: &str = "two id:01J9K3H5Z7Q8X2M4N6P8R0T2V5";

    #[test]
    fn keep_insert_delete_by_content() {
        assert_eq!(diff_lines(&f("a\nb\n"), &f("a\nb\n")), [LineDiff::Keep { from: 0, to: 0 }, LineDiff::Keep { from: 1, to: 1 }]);
        assert_eq!(diff_lines(&f("a\nc\n"), &f("a\nb\nc\n")), [LineDiff::Keep { from: 0, to: 0 }, LineDiff::Insert { to: 1 }, LineDiff::Keep { from: 1, to: 2 }]);
        assert_eq!(diff_lines(&f("a\nb\nc\n"), &f("a\nc\n")), [LineDiff::Keep { from: 0, to: 0 }, LineDiff::Delete { from: 1 }, LineDiff::Keep { from: 2, to: 1 }]);
        assert_eq!(diff_lines(&f(""), &f("a\n")), [LineDiff::Insert { to: 0 }]);
    }

    #[test]
    fn ids_give_change_and_move() {
        let a = f(&alloc::format!("{A}\n{B}\n"));
        let edited = f(&alloc::format!("one edited id:01J9K3H5Z7Q8X2M4N6P8R0T2V4\n{B}\n"));
        assert_eq!(diff_lines(&a, &edited), [LineDiff::Change { from: 0, to: 0 }, LineDiff::Keep { from: 1, to: 1 }]);
        let swapped = f(&alloc::format!("{B}\n{A}\n"));
        assert_eq!(diff_lines(&a, &swapped), [LineDiff::Move { from: 0, to: 1 }, LineDiff::Keep { from: 1, to: 0 }]);
        let stripped = f("one\ntwo\n");
        assert_eq!(diff_lines(&a, &stripped).iter().filter(|d| matches!(d, LineDiff::Keep { .. })).count(), 0, "without ids, content differs");
    }

    #[test]
    fn text_edits_are_char_runs() {
        assert_eq!(diff_text("abc", "abc"), []);
        assert_eq!(diff_text("abc", "abXYc"), [TextEdit::Insert { at: 2, text: "XY".into() }]);
        assert_eq!(diff_text("abXYc", "abc"), [TextEdit::Delete { at: 2, len: 2 }]);
        assert_eq!(diff_text("买菜", "买好菜"), [TextEdit::Insert { at: 1, text: "好".into() }], "char, not byte, positions");
        assert_eq!(diff_text("", "ab"), [TextEdit::Insert { at: 0, text: "ab".into() }]);
        assert_eq!(diff_text("ab", ""), [TextEdit::Delete { at: 0, len: 2 }]);
    }
}
