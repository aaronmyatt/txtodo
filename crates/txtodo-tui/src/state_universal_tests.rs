//! `state_universal.rs`'s tests, over a hand-built task list.

use super::*;

const TODAY: &str = "2026-09-25";

/// A task in workspace `ws`: its priority, due date and contexts read off `raw` by hand.
fn task(ws: &str, raw: &str) -> UTask {
    let word = |prefix: &str| {
        raw.split_whitespace()
            .filter_map(|w| w.strip_prefix(prefix))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    UTask {
        workspace_id: format!("id-{ws}"),
        workspace: ws.to_owned(),
        root_list: "todo.txt".to_owned(),
        raw: raw.to_owned(),
        done: raw.starts_with("x "),
        priority: raw.strip_prefix('(').and_then(|r| r.chars().next()),
        due: word("due:").pop(),
        contexts: word("@"),
        ..UTask::default()
    }
}

fn view() -> UniversalView {
    UniversalView {
        tasks: vec![
            task("notes", "(A) call mum @phone due:2026-09-24"),
            task("notes", "x file taxes"),
            task("work", "(B) ship it @desk due:2026-09-27"),
            task("work", "tidy @desk"),
        ],
        ..UniversalView::default()
    }
}

#[test]
fn rows_group_by_priority_and_hide_done_until_asked() {
    let mut v = view();
    let groups: Vec<String> = v.groups("", TODAY).into_iter().map(|(n, _)| n).collect();
    assert_eq!(groups, ["(A)", "(B)", "No priority"]);
    assert_eq!(v.rows("", TODAY), [0, 2, 3]);
    assert_eq!(v.rows("is:done", TODAY), [1], "is:done shows done rows");
    v.show_done = true;
    assert_eq!(v.rows("", TODAY).len(), 4);
}

#[test]
fn stats_count_overdue_this_week_open_and_done() {
    assert_eq!(view().stats(TODAY), [1, 1, 3, 1]);
}

#[test]
fn filters_narrow_the_rows_and_the_last_workspace_stays_on() {
    let mut v = view();
    v.toggle_context("desk");
    assert_eq!(v.rows("", TODAY), [2, 3]);
    assert_eq!(v.contexts("")[0], ("desk".to_owned(), 2));
    v.toggle_context("desk");
    v.toggle_workspace("id-work");
    assert_eq!(v.rows("", TODAY), [0]);
    v.toggle_workspace("id-notes");
    assert_eq!(v.rows("", TODAY), [0], "the last workspace on stays on");
    assert_eq!(
        v.rows("ship", TODAY),
        Vec::<usize>::new(),
        "search narrows too"
    );
    v.reset();
    assert_eq!(v.rows("", TODAY).len(), 3);
}

#[test]
fn workspaces_list_in_order_with_open_counts() {
    let names: Vec<(String, usize)> = view()
        .workspaces()
        .into_iter()
        .map(|(_, n, open)| (n, open))
        .collect();
    assert_eq!(names, [("notes".to_owned(), 1), ("work".to_owned(), 2)]);
}
