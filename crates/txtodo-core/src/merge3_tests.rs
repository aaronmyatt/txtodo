//! Tests for `merge3.rs`: the cases a todo list meets, then the properties that must hold for any
//! three texts (the set of lines does not depend on which side is ours, no added line is lost, an
//! unchanged side leaves the other as it was).

use super::*;
use proptest::prelude::*;

fn text(lines: &[&str]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn merged(base: &[&str], ours: &[&str], theirs: &[&str]) -> Vec<String> {
    let out = merge3(&text(base), &text(ours), &text(theirs));
    out.lines().map(str::to_owned).collect()
}

#[test]
fn nothing_changed_nothing_moves() {
    let all = ["a", "b", "c"];
    assert_eq!(merged(&all, &all, &all), all);
    assert_eq!(merge3("", "", ""), "");
}

#[test]
fn a_line_added_on_either_side_is_kept() {
    let base = ["a", "b"];
    assert_eq!(
        merged(&base, &["a", "b", "ours"], &base),
        ["a", "b", "ours"]
    );
    assert_eq!(
        merged(&base, &base, &["a", "b", "theirs"]),
        ["a", "b", "theirs"]
    );
    assert_eq!(
        merged(&base, &["a", "b", "ours"], &["a", "b", "theirs"]),
        ["a", "b", "theirs", "ours"].map(str::to_owned),
        "both kept; theirs goes after the line before it, so before ours' own tail"
    );
}

#[test]
fn theirs_additions_land_after_their_predecessor_in_ours_order() {
    let base = ["a", "b", "c"];
    let ours = ["a", "b", "c"];
    let theirs = ["a", "NEW", "b", "c"];
    assert_eq!(merged(&base, &ours, &theirs), ["a", "NEW", "b", "c"]);
    // A new first line goes first.
    assert_eq!(
        merged(&base, &ours, &["TOP", "a", "b", "c"]),
        ["TOP", "a", "b", "c"]
    );
}

#[test]
fn a_deletion_stands_when_the_other_side_left_the_line_alone() {
    let base = ["a", "b", "c"];
    assert_eq!(merged(&base, &["a", "c"], &base), ["a", "c"]);
    assert_eq!(merged(&base, &base, &["a", "c"]), ["a", "c"]);
    // Deleted on both sides: gone once, not twice.
    assert_eq!(merged(&base, &["a", "c"], &["a", "c"]), ["a", "c"]);
}

#[test]
fn completing_a_line_on_one_side_leaves_the_done_line_and_not_the_open_one() {
    let base = ["call mum", "buy milk"];
    let ours = ["buy milk", "x 2026-10-25 call mum"];
    let theirs = ["call mum", "buy milk", "new thing"];
    let got = merged(&base, &ours, &theirs);
    assert!(got.contains(&"x 2026-10-25 call mum".to_owned()), "{got:?}");
    assert!(!got.contains(&"call mum".to_owned()), "{got:?}");
    assert!(got.contains(&"new thing".to_owned()), "{got:?}");
}

#[test]
fn a_line_edited_on_both_sides_keeps_both_edits_and_not_the_old_one() {
    let base = ["call mum", "z"];
    let got = merged(&base, &["call mum +family", "z"], &["call mum @phone", "z"]);
    assert!(got.contains(&"call mum +family".to_owned()), "{got:?}");
    assert!(got.contains(&"call mum @phone".to_owned()), "{got:?}");
    assert!(!got.contains(&"call mum".to_owned()), "{got:?}");
    assert_eq!(got.len(), 3);
}

#[test]
fn an_edit_on_one_side_beats_a_deletion_on_the_other() {
    let base = ["call mum", "z"];
    let got = merged(&base, &["z"], &["call mum @phone", "z"]);
    assert_eq!(
        got.iter().filter(|l| *l == "call mum @phone").count(),
        1,
        "{got:?}"
    );
    assert!(!got.contains(&"call mum".to_owned()), "{got:?}");
}

#[test]
fn the_same_line_added_on_both_sides_is_one_line() {
    let base = ["a"];
    let got = merged(&base, &["a", "buy milk"], &["a", "buy milk"]);
    assert_eq!(got, ["a", "buy milk"]);
}

#[test]
fn repeated_lines_are_counted_not_collapsed() {
    let base = ["x", "x", "y"];
    // Ours deletes one x, theirs adds another: 2 - 1 + 1 = 2.
    let got = merged(&base, &["x", "y"], &["x", "x", "x", "y"]);
    assert_eq!(got.iter().filter(|l| *l == "x").count(), 2, "{got:?}");
}

#[test]
fn an_unchanged_side_leaves_the_other_file_exactly_as_it_was() {
    let base = "a\nb\nc\n";
    let ours = "c\na\nb\nnew\n"; // reordered and added
    assert_eq!(merge3(base, ours, base), ours);
    assert_eq!(merge3(base, base, ours).lines().count(), 4);
}

#[test]
fn the_trailing_newline_follows_either_side_and_blank_lines_are_lines() {
    assert_eq!(merge3("a", "a\nb", "a"), "a\nb");
    assert_eq!(merge3("a\n", "a\nb", "a\n"), "a\nb\n");
    assert_eq!(merge3("a\n\nb\n", "a\n\nb\nc\n", "a\n\nb\n"), "a\n\nb\nc\n");
    assert_eq!(merge3("\n", "\n", "\n"), "\n");
}

fn pool() -> impl Strategy<Value = Vec<&'static str>> {
    let line = prop::sample::select(vec![
        "a",
        "b",
        "c",
        "d",
        "e",
        "x 2026-10-25 a",
        "(A) b +p",
        "",
    ]);
    prop::collection::vec(line, 0..8)
}

fn sorted(lines: Vec<String>) -> Vec<String> {
    let mut lines = lines;
    lines.sort();
    lines
}

proptest! {
    /// Which side is "ours" changes the order, never the lines.
    #[test]
    fn the_lines_do_not_depend_on_which_side_is_ours(b in pool(), o in pool(), t in pool()) {
        let ab = merged(&b, &o, &t);
        let ba = merged(&b, &t, &o);
        prop_assert_eq!(sorted(ab), sorted(ba));
    }

    /// A line either side added is in the result; nothing is in it that neither side had.
    #[test]
    fn no_added_line_is_lost_and_none_is_invented(b in pool(), o in pool(), t in pool()) {
        let got = merged(&b, &o, &t);
        let count = |side: &[&str], k: &str| side.iter().filter(|l| **l == k).count();
        for k in o.iter().chain(&t) {
            let added = count(&o, k).saturating_sub(count(&b, k)).max(count(&t, k).saturating_sub(count(&b, k)));
            let have = got.iter().filter(|l| *l == k).count();
            prop_assert!(have >= added, "{k:?}: {have} < {added} in {got:?}");
            prop_assert!(have <= count(&o, k).max(count(&t, k)).max(count(&b, k)));
        }
        for l in &got {
            prop_assert!(o.contains(&l.as_str()) || t.contains(&l.as_str()), "{l:?} invented");
        }
    }

    /// A side that changed nothing hands the other side's file back as it was.
    #[test]
    fn an_unchanged_side_returns_the_other_exactly(b in pool(), o in pool()) {
        let (base, ours) = (text(&b), text(&o));
        prop_assert_eq!(merge3(&base, &ours, &base), ours.clone());
    }
}
