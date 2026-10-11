// Integration-style unit tests are tests: clippy.toml allows unwrap/expect in #[test] fns.
use super::*;

fn op<'a>(
    path: &'a str,
    source: &'a str,
    kind: &'a str,
    task: &'a str,
    summary: &'a str,
) -> OpFacts<'a> {
    OpFacts {
        path,
        source,
        kind,
        task_id: task,
        summary,
    }
}

const DONE: &str = "Completed = Bool(true)";

#[test]
fn verbs_come_from_kind_and_summary() {
    assert_eq!(verb_of("set_field", DONE), Some(Verb::Completed));
    assert_eq!(
        verb_of("set_field", "Deleted = Bool(true)"),
        Some(Verb::Deleted)
    );
    assert_eq!(
        verb_of("set_field", "Completed = Bool(false)"),
        Some(Verb::Edited),
        "reopened"
    );
    assert_eq!(
        verb_of("set_field", "Priority = Text(\"A\")"),
        Some(Verb::Edited)
    );
    assert_eq!(
        verb_of("set_field", "CompletionDate = Text(\"2026-10-11\")"),
        None
    );
    assert_eq!(
        verb_of("edit_text", "description: 1 edit(s)"),
        Some(Verb::Edited)
    );
    for quiet in [
        "insert",
        "move",
        "blank_insert",
        "blank_remove",
        "notes_edit",
    ] {
        assert_eq!(verb_of(quiet, ""), None, "{quiet}");
    }
}

#[test]
fn own_ops_never_count() {
    let mut d = Debouncer::new("desktop");
    assert!(!d.push(0, op("todo.txt", "desktop", "set_field", "T1", DONE)));
    assert_eq!(d.next_due(), None);
    assert_eq!(d.due(60_000), None);
}

#[test]
fn one_task_from_an_agent() {
    let mut d = Debouncer::new("desktop");
    assert!(d.push(1_000, op("todo.txt", "mcp", "set_field", "T1", DONE)));
    assert!(!d.push(
        1_000,
        op(
            "todo.txt",
            "mcp",
            "set_field",
            "T1",
            "CompletionDate = Text(\"x\")"
        )
    ));
    assert_eq!(d.next_due(), Some(1_000 + QUIET_MS));
    assert_eq!(
        d.due(1_000 + QUIET_MS - 1),
        None,
        "not quiet long enough yet"
    );
    let n = d.due(1_000 + QUIET_MS).unwrap();
    assert_eq!(n.title, "A task was completed by an agent");
    assert_eq!(n.body, "in todo.txt");
    assert_eq!(d.next_due(), None, "the burst starts over");
}

#[test]
fn a_burst_is_one_notice_with_counts() {
    let mut d = Debouncer::new("tui");
    d.push(0, op("todo.txt", "sync", "edit_text", "T1", ""));
    d.push(500, op("todo.txt", "sync", "set_field", "T1", DONE));
    d.push(
        900,
        op(
            "tasks/a/todo.txt",
            "sync",
            "set_field",
            "T2",
            "Deleted = Bool(true)",
        ),
    );
    d.push(1_200, op("tasks/a/todo.txt", "sync", "edit_text", "T3", ""));
    let n = d.due(1_200 + QUIET_MS).unwrap();
    assert_eq!(n.title, "3 tasks changed on another device");
    assert_eq!(
        n.body,
        "1 completed, 1 deleted, 1 edited, in tasks/a/todo.txt and todo.txt"
    );
}

#[test]
fn mixed_sources_say_no_one_and_many_lists_are_counted() {
    let mut d = Debouncer::new("tui");
    d.push(0, op("a.txt", "mcp", "edit_text", "T1", ""));
    d.push(0, op("b.txt", "cli", "edit_text", "T2", ""));
    d.push(0, op("c.txt", "cli", "edit_text", "T3", ""));
    let n = d.due(QUIET_MS).unwrap();
    assert_eq!(n.title, "3 tasks changed");
    assert_eq!(n.body, "3 edited, in a.txt and 2 more lists");
}

#[test]
fn a_burst_that_never_goes_quiet_still_reports() {
    let mut d = Debouncer::new("desktop");
    let mut now = 0;
    while now < MAX_WAIT_MS {
        d.push(now, op("todo.txt", "sync", "edit_text", "T1", ""));
        now += 1_000;
    }
    assert_eq!(d.next_due(), Some(MAX_WAIT_MS));
    assert!(d.due(MAX_WAIT_MS).is_some());
}
