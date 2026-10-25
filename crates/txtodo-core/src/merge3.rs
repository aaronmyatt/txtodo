//! A three-way merge of todo.txt text, line by line, that never loses a line (task
//! `two-kinds-of-workspace`, ADR 0043): what `txtodo merge-driver` runs when git merges a repo's
//! `todo.txt`. Pure: text in, text out.
//!
//! A todo.txt line has no identity in the file, so lines are compared by their exact text and an
//! edit is a line leaving and a new one arriving. Per distinct line `K`, with `b`, `o`, `t` copies in
//! the base, ours and theirs:
//!
//! - a side that has fewer copies than the base **deleted** `b - side` of them; a deletion by either
//!   side stands (the same deletion on both sides counts once);
//! - a side that has more copies than the base **added** `side - b`; an addition on either side
//!   stands (the same line added on both sides counts once, as git treats a change both sides made);
//! - so the result holds `b - max(b - o, b - t) + max(o - b, t - b)` copies, floored at zero.
//!
//! That gives the rules a todo list needs. Added on either side: kept. Completed on one side
//! (`do` turns the open line into a new `x …` line at the bottom): the open line is gone and the
//! done one is there. Edited on both: the old line is gone and **both** edits stay, two lines, since
//! nothing says one replaces the other. Edited on one side and deleted on the other: the edit stays.
//! The set of lines is the same whichever side is "ours"; only their order follows `ours`.
//!
//! Order: the result follows `ours`. A line `theirs` added goes right after the nearest line before
//! it in `theirs` that the result has, or first when none is. So a side that is unchanged since the
//! base leaves the other side's file exactly as it was.
//! Ref: <https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver>

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Merges `ours` and `theirs`, two edits of `base`. See the module doc for the rules.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> String {
    let (b, o, t) = (lines(base), lines(ours), lines(theirs));
    let (nb, no, nt) = (counts(&b), counts(&o), counts(&t));
    let want = |line: &str| {
        let n = |m: &BTreeMap<&str, usize>| m.get(line).copied().unwrap_or(0);
        let (b, o, t) = (n(&nb), n(&no), n(&nt));
        let deleted = b.saturating_sub(o).max(b.saturating_sub(t));
        let added = o.saturating_sub(b).max(t.saturating_sub(b));
        (b - deleted.min(b)) + added
    };
    // Ours first, in its order, up to the copies the result wants.
    let mut out: Vec<&str> = Vec::new();
    let mut taken: BTreeMap<&str, usize> = BTreeMap::new();
    for &line in &o {
        let used = taken.entry(line).or_insert(0);
        if *used < want(line) {
            *used += 1;
            out.push(line);
        }
    }
    // Then what theirs has that the result wants and ours did not supply, after its predecessor.
    let mut owed: BTreeMap<&str, usize> = BTreeMap::new();
    for &line in nt.keys() {
        let missing = want(line).saturating_sub(taken.get(line).copied().unwrap_or(0));
        if missing > 0 {
            owed.insert(line, missing);
        }
    }
    insert_theirs(&mut out, &t, &mut owed);
    join(&out, ours, theirs)
}

/// Puts the `owed` copies of lines `theirs` has into `out`, each after the nearest earlier line of
/// `theirs` that `out` has. Of a line's copies in `theirs` the last ones are the extra ones.
fn insert_theirs<'a>(
    out: &mut Vec<&'a str>,
    theirs: &[&'a str],
    owed: &mut BTreeMap<&'a str, usize>,
) {
    // Which copies of each owed line are extra: counting from the end of `theirs`.
    let mut from_end: BTreeMap<&str, usize> = BTreeMap::new();
    let mut extra = alloc::vec![false; theirs.len()];
    for (i, &line) in theirs.iter().enumerate().rev() {
        let seen = from_end.entry(line).or_insert(0);
        if *seen < owed.get(line).copied().unwrap_or(0) {
            extra[i] = true;
        }
        *seen += 1;
    }
    // The slot just after which the last line placed (or each earlier line `out` already had) sits.
    let mut after: Option<usize> = None;
    for (i, &line) in theirs.iter().enumerate() {
        if extra[i] {
            let at = after.map_or(0, |a| a + 1);
            out.insert(at, line);
            after = Some(at);
        } else if let Some(pos) = out.iter().rposition(|&l| l == line) {
            after = Some(pos);
        }
    }
}

/// The lines of `text`: split on `\n`, one trailing newline not counting as a last empty line.
fn lines(text: &str) -> Vec<&str> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    if body.is_empty() && text.is_empty() {
        return Vec::new();
    }
    body.split('\n').collect()
}

fn counts<'a>(lines: &[&'a str]) -> BTreeMap<&'a str, usize> {
    let mut map = BTreeMap::new();
    for &line in lines {
        *map.entry(line).or_insert(0) += 1;
    }
    map
}

/// `out` as text, ending in a newline when either side's file did.
fn join(out: &[&str], ours: &str, theirs: &str) -> String {
    if out.is_empty() {
        return String::new();
    }
    let mut text = out.join("\n");
    if ours.ends_with('\n') || theirs.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[cfg(test)]
#[path = "merge3_tests.rs"]
mod tests;
