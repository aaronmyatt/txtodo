//! `detail.rs`'s tests: the panel drawn on a test terminal.

use super::*;
use crate::state::LineState;
use crate::state_detail::{Doc, Parent};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn drawn(state: &AppState) -> (Vec<String>, HitMap) {
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap_or_else(|e| panic!("{e}"));
    let mut hits = HitMap::default();
    terminal
        .draw(|f| draw(f, f.area(), state, &mut hits))
        .unwrap_or_else(|e| panic!("{e}"));
    let rows = terminal
        .backend()
        .buffer()
        .content()
        .chunks(60)
        .map(|r| r.iter().map(|c| c.symbol()).collect())
        .collect();
    (rows, hits)
}

#[test]
fn the_panel_shows_crumbs_parent_sub_list_and_notes() {
    let mut state = AppState::fixture();
    state.shell.root = "/home/u/notes".to_owned();
    state.detail.levels.push(Level {
        parent: Parent {
            path: "todo.txt".to_owned(),
            line_number: 4,
            raw: "Water the plants +home @garden".to_owned(),
            ..Parent::default()
        },
        dir: "tasks/plants".to_owned(),
        doc: Doc {
            path: "tasks/plants/todo.txt".to_owned(),
            lines: vec![LineState::from_raw(1, "x fill the can")],
            ..Doc::default()
        },
        ..Level::default()
    });
    state.detail.levels[0].notes.text = "ferns first".to_owned();
    let (rows, hits) = drawn(&state);
    let all = rows.join("\n");
    assert!(
        rows[1].contains("notes \u{203a}  Water the plants +home"),
        "{all}"
    );
    assert!(rows[2].contains("Parent  Water the plants"), "{all}");
    assert!(
        rows[2].contains("Mark done"),
        "every sub-task is done: {all}"
    );
    assert!(all.contains("1 x fill the can"), "{all}");
    assert!(all.contains("ferns first"), "{all}");
    assert_eq!(hits.at(2, 1), Some(Target::Command(Command::DetailClose)));
    assert_eq!(hits.at(20, 1), Some(Target::Crumb(1)));
    let sub_row = u16::try_from(
        rows.iter()
            .position(|r| r.contains("fill the can"))
            .unwrap_or(0),
    )
    .unwrap_or(0);
    assert_eq!(hits.at(5, sub_row), Some(Target::DetailRow(0)));
}
