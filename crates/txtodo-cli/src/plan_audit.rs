//! An exhaustive audit of `daemon_mode::plan_mutations` over every pair of small files, both
//! shapes the daemon hands the CLI: sidecar text (no `id:`, lines matched by content) and tagged
//! text (every line an `id:`, plain or done so `archive_plan`'s shapes are covered too). For every
//! (old, new) that gets a plan:
//! - it replays to `new` exactly (`plan_check::reproduces`, which `plan_mutations` itself
//!   enforces: this re-asserts that nothing bypasses it);
//! - it never holds both a `Delete` and an `Add`: that pair is how a rewritten or moved line
//!   looks, and the daemon would tombstone the old task and mint a new one, losing its history and
//!   any concurrent edit another device made to it. Those diffs go out as a whole-document
//!   `Replace`, where identity is matched by id or fingerprint.
//!
//! And the other direction, so the fallback stays a fallback: in tagged text, changing one line in
//! place (same ids, same order) must stay an id-guarded `Edit`, never a `Replace`.

use crate::daemon_mode::plan_mutations;
use crate::plan_check::reproduces;
use txtodo_core::{File, parse_file};
use txtodo_proto::v1::mutation::Kind;

type Lines = Vec<String>;

fn file(lines: &[String]) -> File {
    let text = if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    };
    parse_file(text.as_bytes())
}

/// Every file of up to 3 lines over `alphabet` that `ok` accepts.
fn files(alphabet: &[String], ok: impl Fn(&[String]) -> bool) -> Vec<Lines> {
    let mut out: Vec<Lines> = vec![Vec::new()];
    let mut layer: Vec<Lines> = vec![Vec::new()];
    for _ in 0..3 {
        layer = layer
            .iter()
            .flat_map(|f| {
                alphabet
                    .iter()
                    .map(move |l| [f.clone(), vec![l.clone()]].concat())
            })
            .filter(|f| ok(f))
            .collect();
        out.extend(layer.clone());
    }
    out
}

fn tag(line: &str) -> Option<&str> {
    line.rsplit_once(" id:").map(|(_, id)| id)
}

/// Tagged text: task `n` plain or done, or a blank. A task never repeats (ids are unique).
fn tagged_files() -> Vec<Lines> {
    let mut alphabet = vec![String::new()];
    for (i, n) in ['a', 'b', 'c'].iter().enumerate() {
        alphabet.push(format!("{n} id:01ARZ3NDEKTSV4RRFFQ69G5FA{i}"));
        alphabet.push(format!("x 2026-09-19 {n} id:01ARZ3NDEKTSV4RRFFQ69G5FA{i}"));
    }
    files(&alphabet, |f| {
        let ids: Vec<&str> = f.iter().filter_map(|l| tag(l)).collect();
        ids.iter().enumerate().all(|(i, x)| !ids[..i].contains(x))
    })
}

/// Sidecar text: no ids, lines are just their content; blanks and repeats allowed.
fn sidecar_files() -> Vec<Lines> {
    let alphabet: Lines = ["a", "b", "c", "x a", ""].map(String::from).to_vec();
    files(&alphabet, |_| true)
}

/// The same tasks in the same order, exactly one of them changed (done, or not).
fn one_line_changed(old: &[String], new: &[String]) -> bool {
    let same_ids = old.len() == new.len() && old.iter().zip(new).all(|(o, n)| tag(o) == tag(n));
    same_ids && old.iter().zip(new).filter(|(o, n)| o != n).count() == 1
}

fn audit(shape: &str, set: &[Lines], in_place_stays_a_mutation: bool) {
    let (mut planned, mut churn, mut inexact, mut fell_back) =
        (0, Vec::new(), Vec::new(), Vec::new());
    for old in set {
        for new in set {
            let case = format!("{old:?} -> {new:?}");
            let Some(muts) = plan_mutations(&file(old), &file(new)) else {
                if in_place_stays_a_mutation && one_line_changed(old, new) {
                    fell_back.push(case);
                }
                continue;
            };
            planned += 1;
            let has = |f: fn(&Kind) -> bool| muts.iter().filter_map(|m| m.kind.as_ref()).any(f);
            if has(|k| matches!(k, Kind::Delete(_))) && has(|k| matches!(k, Kind::Add(_))) {
                churn.push(case.clone());
            }
            if !reproduces(&file(old), &file(new), &muts) {
                inexact.push(case);
            }
        }
    }
    let pairs = set.len() * set.len();
    eprintln!("{shape}: {pairs} pairs, {planned} planned as mutations, the rest as Replace");
    for (what, cases) in [
        ("delete+add", &churn),
        ("inexact", &inexact),
        ("in place fell back", &fell_back),
    ] {
        cases
            .iter()
            .take(4)
            .for_each(|c| eprintln!("  {what}: {c}"));
        assert!(cases.is_empty(), "{shape}: {} pairs: {what}", cases.len());
    }
}

#[test]
fn sidecar_text_every_plan_is_exact_and_never_a_delete_plus_add() {
    audit("sidecar", &sidecar_files(), false);
}

#[test]
fn tagged_text_every_plan_is_exact_and_an_in_place_change_stays_an_edit() {
    audit("tagged", &tagged_files(), true);
}
