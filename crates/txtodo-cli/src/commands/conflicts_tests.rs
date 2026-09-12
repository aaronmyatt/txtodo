//! Unit tests for the `conflicts` command: the list rendering (text and JSON) and the
//! side-to-wire-enum mapping. A child module of `conflicts`, so the private helpers are visible.

use super::*;

fn flag(line_number: u32, mine: &str, theirs: &str) -> pb::ReviewFlag {
    pb::ReviewFlag {
        task_id: "01TEST".to_owned(),
        line_number,
        mine: mine.to_owned(),
        theirs: theirs.to_owned(),
        raised_at_ms: 1_700_000_000_000,
    }
}

#[test]
fn text_block_shows_the_line_and_a_two_line_unified_diff() {
    let f = flag(3, "buy milk", "buy oat milk");
    let block = flag_text(&f, "buy oat milk");
    assert!(block.contains("line 3"), "{block}");
    assert!(block.contains("--- mine (this device)"), "{block}");
    assert!(block.contains("+++ theirs (the other device)"), "{block}");
    assert!(block.contains("- buy milk"), "{block}");
    assert!(block.contains("+ buy oat milk"), "{block}");
}

#[test]
fn a_task_no_longer_in_the_file_says_so() {
    let f = flag(0, "buy milk", "buy oat milk");
    let block = flag_text(&f, "(no longer in the file)");
    assert!(block.contains("gone from the file"), "{block}");
    assert!(!block.contains("line 0"), "{block}");
}

#[test]
fn json_escapes_quotes_and_newlines_in_the_descriptions() {
    let f = flag(2, "say \"hi\"", "leave\na note");
    let out = flag_json(&f, "say \"hi\"");
    assert!(out.contains(r#""mine":"say \"hi\"""#), "{out}");
    assert!(out.contains(r#""theirs":"leave\na note""#), "{out}");
    assert!(out.contains(r#""line":2"#), "{out}");
    assert!(out.contains(r#""raised_at_ms":1700000000000"#), "{out}");
}

#[test]
fn side_words_and_wire_values_stay_aligned() {
    // The wire enum and the CLI words must never drift apart: each side maps once.
    for (side, word, wire_value) in [
        (Side::Mine, "mine", pb::Resolution::Mine),
        (Side::Theirs, "theirs", pb::Resolution::Theirs),
        (Side::Merged, "merged", pb::Resolution::Merged),
    ] {
        assert_eq!(side.word(), word);
        assert_eq!(wire(side), wire_value);
    }
}
