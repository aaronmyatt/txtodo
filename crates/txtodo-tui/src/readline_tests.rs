//! `readline.rs`'s tests. A field is written with `|` for the caret.

use super::*;

/// Applies `ops` to `field` (caret at `|`) with `kill`, and returns it the same way.
fn run_with(field: &str, ops: &[Op], kill: &mut Kill) -> String {
    let mut caret = field
        .find('|')
        .unwrap_or_else(|| panic!("no caret in {field:?}"));
    let mut text = field.replacen('|', "", 1);
    for op in ops {
        apply(*op, &mut text, &mut caret, kill);
    }
    text.insert(caret, '|');
    text
}

fn run(field: &str, ops: &[Op]) -> String {
    run_with(field, ops, &mut Kill::default())
}

#[test]
fn line_motions_stay_on_the_logical_line() {
    assert_eq!(run("call m|um", &[Op::LineStart]), "|call mum");
    assert_eq!(run("call m|um", &[Op::LineEnd]), "call mum|");
    assert_eq!(
        run("one\ntw|o\nthree", &[Op::LineStart]),
        "one\n|two\nthree"
    );
    assert_eq!(run("one\ntw|o\nthree", &[Op::LineEnd]), "one\ntwo|\nthree");
}

#[test]
fn words_are_letters_and_digits_so_todo_tokens_take_several_hops() {
    let line = "|call +home due:2026-10-09";
    assert_eq!(run(line, &[Op::WordForward]), "call| +home due:2026-10-09");
    assert_eq!(
        run(line, &[Op::WordForward; 2]),
        "call +home| due:2026-10-09"
    );
    assert_eq!(
        run(line, &[Op::WordForward; 3]),
        "call +home due|:2026-10-09"
    );
    assert_eq!(run("call +ho|me", &[Op::WordBack]), "call +|home");
    assert_eq!(run("call +|home", &[Op::WordBack]), "|call +home");
    assert_eq!(run("|", &[Op::WordBack, Op::WordForward]), "|");
}

#[test]
fn wide_characters_and_emoji_are_words_or_not_by_their_letters() {
    assert_eq!(
        run("|日本 x", &[Op::WordForward]),
        "日本| x",
        "a run of ideographs is one word"
    );
    assert_eq!(
        run("list.last|", &[Op::WordBack]),
        "list.|last",
        "a dot ends a word"
    );
    assert_eq!(
        run("a 👍🏽 |b", &[Op::WordBack]),
        "|a 👍🏽 b",
        "an emoji is not a word"
    );
    assert_eq!(
        run("café| x", &[Op::WordBack]),
        "|café x",
        "an accent stays in its word"
    );
}

#[test]
fn kills_go_to_one_slot_that_joins_while_they_follow_each_other() {
    let mut kill = Kill::default();
    let line = run_with("call mum +home|", &[Op::KillWordBack], &mut kill);
    assert_eq!((line.as_str(), kill.text.as_str()), ("call mum +|", "home"));
    let line = run_with(&line, &[Op::KillWordBack], &mut kill);
    assert_eq!(
        (line.as_str(), kill.text.as_str()),
        ("call |", "mum +home"),
        "a second backward kill goes in front"
    );
    let line = run_with(&line, &[Op::CharForward, Op::KillLineBack], &mut kill);
    assert_eq!(
        (line.as_str(), kill.text.as_str()),
        ("|", "call "),
        "a motion in between starts the slot over"
    );
    let line = run_with(&line, &[Op::Yank, Op::Yank], &mut kill);
    assert_eq!(line, "call call |");
}

#[test]
fn ctrl_w_kills_back_to_a_space_and_alt_d_forward_to_a_word_end() {
    assert_eq!(
        run("call due:2026-10-09|", &[Op::KillSpaceWordBack]),
        "call |"
    );
    assert_eq!(run("call mum  |", &[Op::KillSpaceWordBack]), "call |");
    assert_eq!(run("|call +home", &[Op::KillWordForward]), "| +home");
}

#[test]
fn ctrl_k_kills_to_the_end_and_at_the_end_the_line_break() {
    let mut kill = Kill::default();
    let notes = run_with("one|\ntwo", &[Op::KillLineForward], &mut kill);
    assert_eq!(notes, "one|two");
    let notes = run_with(&notes, &[Op::KillLineForward], &mut kill);
    assert_eq!((notes.as_str(), kill.text.as_str()), ("one|", "\ntwo"));
    assert_eq!(
        run("one|", &[Op::KillLineForward]),
        "one|",
        "nothing after: nothing"
    );
}

#[test]
fn characters_move_delete_and_swap() {
    assert_eq!(run("ab|c", &[Op::CharBack]), "a|bc");
    assert_eq!(run("ab|c", &[Op::CharForward]), "abc|");
    assert_eq!(run("ab|c", &[Op::DeleteForward]), "ab|");
    assert_eq!(run("ab|c", &[Op::DeleteBack]), "a|c");
    assert_eq!(
        run("|", &[Op::DeleteForward]),
        "|",
        "an empty field: nothing"
    );
    assert_eq!(run("ab|cd", &[Op::TransposeChars]), "acb|d");
    assert_eq!(
        run("abcd|", &[Op::TransposeChars]),
        "abdc|",
        "at the end, the last two"
    );
    assert_eq!(run("|ab", &[Op::TransposeChars]), "|ab", "nothing before");
}

#[test]
fn words_swap_and_change_case_from_the_caret() {
    assert_eq!(run("call| mum", &[Op::TransposeWords]), "mum call|");
    assert_eq!(
        run("call mum|", &[Op::TransposeWords]),
        "mum call|",
        "at the end, the last two"
    );
    assert_eq!(
        run("|call", &[Op::TransposeWords]),
        "|call",
        "one word: nothing"
    );
    assert_eq!(run("|call mum", &[Op::Upcase]), "CALL| mum");
    assert_eq!(run("call |MUM", &[Op::Downcase]), "call mum|");
    assert_eq!(run("| mUM x", &[Op::Capitalize]), " Mum| x");
}

#[test]
fn undo_steps_back_and_a_run_of_typing_is_one_step() {
    let mut undo = Undo::default();
    undo.record("", 0, true);
    undo.record("a", 1, true);
    undo.record("ab", 2, false);
    assert_eq!(undo.undo(), Some(("ab".to_owned(), 2)));
    assert_eq!(
        undo.undo(),
        Some((String::new(), 0)),
        "the typing run is one step"
    );
    assert_eq!(undo.undo(), None);
    undo.record("x", 1, false);
    undo.clear();
    assert_eq!(undo.undo(), None);
}
