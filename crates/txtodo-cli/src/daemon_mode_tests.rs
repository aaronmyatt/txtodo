//! `daemon_mode.rs`'s unit tests, split out for that file's line budget (same pattern as
//! `client_tests.rs`); `super` is `daemon_mode`.

use super::*;

fn kinds(old: &str, new: &str) -> Option<Vec<String>> {
    plan_mutations(&parse_file(old.as_bytes()), &parse_file(new.as_bytes())).map(|ms| {
        ms.into_iter()
            .map(|m| match m.kind {
                Some(mutation::Kind::Add(a)) => format!("add:{}", a.line),
                Some(mutation::Kind::Edit(e)) => {
                    format!(
                        "edit:{}:{}",
                        e.task.map(|t| t.line_number).unwrap_or(0),
                        e.new_line
                    )
                }
                Some(mutation::Kind::Delete(d)) => {
                    format!(
                        "del:{}:{}",
                        d.task.map(|t| t.line_number).unwrap_or(0),
                        d.leave_blank
                    )
                }
                other => format!("{other:?}"),
            })
            .collect()
    })
}

#[test]
fn appends_edits_and_deletes_are_expressed_bottom_up() {
    assert_eq!(kinds("a\n", "a\nb\nc\n").unwrap(), vec!["add:b", "add:c"]);
    // Daemon-held lines always carry ids; an in-place edit is a same-id Change.
    let a = "a id:01ARZ3NDEKTSV4RRFFQ69G5FAA\n";
    assert_eq!(
        kinds(&format!("{a}b\n"), &format!("(A) {a}b\n")).unwrap(),
        vec![format!("edit:1:(A) {}", a.trim_end())]
    );
    assert_eq!(
        kinds("a\nb\nc\n", "b\n").unwrap(),
        vec!["del:3:false", "del:1:false"]
    );
    assert_eq!(
        kinds("a\nb\n", "\nb\n").unwrap(),
        vec!["del:1:true"],
        "todo.sh del leaves a blank"
    );
    assert_eq!(kinds("a\nb\n", "a\nb\n").unwrap(), Vec::<String>::new());
}

#[test]
fn inexpressible_diffs_fall_back() {
    assert_eq!(kinds("a\n\nb\n", "a\nb\n"), None, "blank removal (archive)");
    assert_eq!(kinds("a\nc\n", "a\nb\nc\n"), None, "mid-file insert");
    assert_eq!(
        kinds("a\n", "a\n\n"),
        None,
        "a trailing blank with no delete to pair with"
    );
}

#[test]
fn the_scratch_copy_takes_the_daemon_documents_file_name() {
    assert_eq!(scratch_doc("todo.txt"), "todo.txt");
    assert_eq!(scratch_doc("work.txt"), "work.txt");
    assert_eq!(scratch_doc("lists/work.txt"), "work.txt");
    assert_eq!(
        scratch_doc(""),
        "todo.txt",
        "an empty path keeps the old name"
    );
}

const T0: &str = "call mum id:01ARZ3NDEKTSV4RRFFQ69G5FA0";
const T1: &str = "walk dog id:01ARZ3NDEKTSV4RRFFQ69G5FA1";

/// Root todo "daemon-mode do sends Edit plus MoveToEnd": `do` (complete, move to the end)
/// goes out as the daemon's own `Complete`, which moves the line itself, so the op log says
/// complete. `do -A` leaves the line in place, which only an `Edit` expresses.
#[test]
fn do_plans_as_one_complete_and_do_a_as_an_edit() {
    let old = parse_file(format!("{T0}\n{T1}\n").as_bytes());
    let done = parse_file(format!("{T1}\nx 2026-09-23 {T0}\n").as_bytes());
    let plan = plan_mutations(&old, &done).unwrap_or_else(|| panic!("plannable"));
    assert_eq!(plan.len(), 1);
    assert!(matches!(
        plan[0].kind,
        Some(mutation::Kind::Complete(pb::Complete { ref today, .. })) if today == "2026-09-23"
    ));
    let in_place = parse_file(format!("x 2026-09-23 {T0}\n{T1}\n").as_bytes());
    let plan = plan_mutations(&old, &in_place).unwrap_or_else(|| panic!("plannable"));
    assert!(matches!(plan[0].kind, Some(mutation::Kind::Edit(_))));
}
