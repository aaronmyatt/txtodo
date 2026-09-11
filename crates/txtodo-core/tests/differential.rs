//! Differential test (plan M1): a second parser generated from `specs/todotxt.abnf` by `abnf_to_pest`
//! must agree with the hand-written strict parser on every corpus line and on 10 000 random lines.
//! The generated grammar is checked in at `tests/todotxt.pest`; `generated_grammar_is_current` regenerates
//! it and fails on drift, so the ABNF, the pest grammar and the hand parser move together.
//!
//! Two mechanical adjustments, documented so nobody wonders:
//! - `abnf_to_pest` predates RFC 7405: `%s"x"` is read with the `%s` stripped, then every pest literal is
//!   made case-sensitive again (`^"` → `"`); no literal in this grammar has letters except those.
//! - pest is PEG: an ordered choice never retries once an alternative succeeds, so the top-level
//!   alternatives of `line` and `incomplete` are each anchored with `~ EOI`. Same language, PEG-safe.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]
// pest_derive generates an undocumented `pub enum Rule` in this test binary.
#![allow(missing_docs)]

use pest::iterators::Pair;
use pest::Parser;
use proptest::prelude::*;
use std::fs;
use std::path::Path;
use txtodo_core::{parse_file, parse_line, Date, LineKind, Mode, Task};

#[derive(pest_derive::Parser)]
#[grammar = "tests/todotxt.pest"]
struct Generated;

const ABNF: &str = include_str!("../../../specs/todotxt.abnf");
const PEST: &str = include_str!("todotxt.pest");

/// Converts the ABNF to the pest text that `tests/todotxt.pest` must contain.
fn generate_pest(abnf: &str) -> String {
    let rules = abnf_to_pest::parse_abnf(&abnf.replace("%s\"", "\"")).expect("valid ABNF");
    let mut out = Vec::new();
    abnf_to_pest::render_rules_to_pest(rules).render(100, &mut out).expect("render");
    let body: Vec<String> = String::from_utf8(out).expect("utf8").replace("^\"", "\"").lines().map(anchor).collect();
    format!(
        "// GENERATED from specs/todotxt.abnf by tests/differential.rs. Do not edit: the test regenerates and diffs.\n\
         {}\n\n// RFC 5234 core rules the ABNF relies on, and the anchored entry point.\nDIGIT = {{ '0'..'9' }}\nSP = {{ \" \" }}\nentry = {{ SOI ~ line }}\n",
        body.join("\n")
    )
}

/// `line` and `incomplete` are CFG alternations; anchor each alternative so PEG cannot commit early.
fn anchor(line: &str) -> String {
    for name in ["line", "incomplete"] {
        let prefix = format!("{name} = {{ ");
        if let Some(body) = line.strip_prefix(&prefix).and_then(|b| b.strip_suffix(" }")) {
            let alts: Vec<String> = body.split(" | ").map(|a| format!("{a} ~ EOI")).collect();
            return format!("{prefix}{} }}", alts.join(" | "));
        }
    }
    line.to_string()
}

#[test]
fn generated_grammar_is_current() {
    assert_eq!(PEST, generate_pest(ABNF), "tests/todotxt.pest is stale: regenerate it from specs/todotxt.abnf");
}

/// What both parsers agree to compare.
#[derive(Debug, PartialEq, Eq)]
struct Fields {
    completed: bool,
    dates: Vec<String>,
    priority: Option<char>,
    description: String,
}

fn hand_fields(t: &Task<'_>) -> Fields {
    Fields {
        completed: t.completed,
        dates: [t.completion_date, t.creation_date].iter().flatten().map(ToString::to_string).collect(),
        priority: t.priority.map(|p| p.as_char()),
        description: t.description.to_string(),
    }
}

/// Walks the pest tree iteratively (depth is bounded by the grammar; asserted ≤ 8).
fn pest_fields(root: Pair<'_, Rule>) -> Fields {
    let mut f = Fields { completed: false, dates: Vec::new(), priority: None, description: String::new() };
    let mut stack: Vec<(Pair<'_, Rule>, usize)> = vec![(root, 0)];
    while let Some((pair, depth)) = stack.pop() {
        assert!(depth <= 8, "grammar depth is small");
        match pair.as_rule() {
            Rule::completed => f.completed = true,
            Rule::date => f.dates.push(pair.as_str().to_string()),
            Rule::priority => f.priority = pair.as_str().chars().nth(1),
            Rule::description => {
                f.description = pair.as_str().to_string();
                continue; // words inside are not prefix fields
            }
            _ => {}
        }
        let children: Vec<_> = pair.into_inner().collect();
        stack.extend(children.into_iter().rev().map(|c| (c, depth + 1)));
    }
    f.dates.sort_by_key(|_| 0); // stable: keep tree order (completion first, then creation)
    f
}

/// The one known divergence: the ABNF fixes only the shape of a date; the hand parser also checks the
/// calendar. Lines whose prefix has a date-shaped word that is not a real date are skipped.
fn has_impossible_prefix_date(raw: &str) -> bool {
    raw.split(' ').take(3).any(|w| w.len() == 10 && w.as_bytes()[4] == b'-' && w.as_bytes()[7] == b'-' && w.bytes().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) && Date::parse(w).is_none())
}

fn compare(raw: &str) -> Result<(), String> {
    if has_impossible_prefix_date(raw) {
        return Ok(());
    }
    let hand = parse_line(raw, Mode::Strict);
    let pest = Generated::parse(Rule::entry, raw);
    match (hand, pest) {
        (Err(e), Ok(_)) => Err(format!("{raw:?}: hand rejects ({e}), pest accepts")),
        (Ok(_), Err(e)) => Err(format!("{raw:?}: hand accepts, pest rejects ({e})")),
        (Err(_), Err(_)) => Ok(()),
        (Ok(line), Ok(mut pairs)) => {
            let expected = match line.kind {
                LineKind::Task(t) => hand_fields(&t),
                LineKind::Blank => Fields { completed: false, dates: vec![], priority: None, description: String::new() },
            };
            let got = pest_fields(pairs.next().expect("entry"));
            if got == expected { Ok(()) } else { Err(format!("{raw:?}: hand {expected:?} vs pest {got:?}")) }
        }
    }
}

#[test]
fn both_parsers_agree_on_the_corpus() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut checked = 0;
    for entry in fs::read_dir(dir).expect("corpus") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        for line in parse_file(&fs::read(&path).expect("read")).lines {
            compare(line.raw().expect("utf8")).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            checked += 1;
        }
    }
    assert!(checked >= 50, "corpus has {checked} lines");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]
    #[test]
    fn both_parsers_agree_on_random_lines(raw in "[xX(A-C)0-9 \\-:+@a-c\t.]{0,24}") {
        compare(&raw).map_err(TestCaseError::fail)?;
    }
}
