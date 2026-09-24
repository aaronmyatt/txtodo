//! `universal.rs`'s tests: the screen drawn on a test terminal.

use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn state() -> AppState {
    let mut state = AppState::fixture();
    let task = |ws: &str, raw: &str, line: u32| UTask {
        workspace_id: format!("id-{ws}"),
        workspace: ws.to_owned(),
        line_number: line,
        raw: raw.to_owned(),
        priority: raw.strip_prefix('(').and_then(|r| r.chars().next()),
        contexts: raw
            .split_whitespace()
            .filter_map(|w| w.strip_prefix('@'))
            .map(str::to_owned)
            .collect(),
        ..UTask::default()
    };
    state.universal.tasks = vec![
        task("notes", "(A) call mum @phone", 1),
        task("work", "tidy the desk @desk", 1),
    ];
    state
}

fn drawn(state: &AppState) -> (Vec<String>, HitMap) {
    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = HitMap::default();
    terminal
        .draw(|f| draw(f, f.area(), state, &mut hits))
        .unwrap_or_else(|e| panic!("{e}"));
    let rows = terminal
        .backend()
        .buffer()
        .content()
        .chunks(80)
        .map(|r| r.iter().map(|c| c.symbol()).collect())
        .collect();
    (rows, hits)
}

#[test]
fn the_screen_lists_stats_filters_and_grouped_rows() {
    let (rows, hits) = drawn(&state());
    let all = rows.join("\n");
    assert!(rows[0].contains("t 2 open"), "{all}");
    assert!(
        rows[1].contains(" Priority ") && rows[1].contains("show done"),
        "{all}"
    );
    assert!(
        rows[2].contains(" notes 1 ") && rows[2].contains(" work 1 "),
        "{all}"
    );
    assert!(rows[3].contains("@desk 1"), "{all}");
    assert!(rows[4].contains("(A) \u{b7} 1"), "a group heading: {all}");
    assert!(
        rows[5].contains("\u{25cb} (A) call mum @phone") && rows[5].contains("[no]"),
        "{all}"
    );
    assert_eq!(hits.at(5, 5), Some(Target::UniversalRow(0)));
    assert_eq!(hits.at(5, 4), None, "a heading is not a row");
    let group = u16::try_from(rows[1].find(" Due ").unwrap_or(0) + 1).unwrap_or(0);
    assert_eq!(hits.at(group, 1), Some(Target::UniversalGroup(1)));
}

#[test]
fn nothing_matching_offers_reset_filters() {
    let mut state = state();
    state.shell.search = "nothing-like-this".to_owned();
    let (rows, hits) = drawn(&state);
    assert!(rows[4].contains("Nothing matches"), "{rows:?}");
    let x = u16::try_from(rows[4].find("Reset filters").unwrap_or(0)).unwrap_or(0);
    assert_eq!(
        hits.at(x, 4),
        Some(Target::Command(Command::UniversalReset))
    );
}
