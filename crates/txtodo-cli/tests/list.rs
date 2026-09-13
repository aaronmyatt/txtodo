//! Listing commands against real files: numbering, sort, footer, --json, listfile lookup.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

fn run(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("txtodo runs: {e}"));
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

const TODO: &str = "(B) 2026-09-11 beta +work @desk\n\n2026-09-11 Alpha +home\nx 2026-09-11 2026-09-10 done @desk\n(A) urgent +work id:01ARZ3NDEKTSV4RRFFQ69G5FAV\n";

#[test]
fn list_numbers_sorts_filters_and_counts_every_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), TODO).unwrap();
    let (_, out) = run(dir.path(), &["ls"]);
    assert_eq!(
        out,
        "5 (A) urgent +work id:01ARZ3NDEKTSV4RRFFQ69G5FAV\n1 (B) 2026-09-11 beta +work @desk\n3 2026-09-11 Alpha +home\n4 x 2026-09-11 2026-09-10 done @desk\n--\nTODO: 4 of 5 tasks shown\n"
    );
    let (_, out) = run(dir.path(), &["list", "@desk", "-done"]);
    assert_eq!(
        out,
        "1 (B) 2026-09-11 beta +work @desk\n--\nTODO: 1 of 5 tasks shown\n"
    );
    let (_, out) = run(dir.path(), &["lsp", "b-c"]);
    assert!(
        out.starts_with("1 (B) ") && out.ends_with("TODO: 1 of 5 tasks shown\n"),
        "{out}"
    );
    // `lsa` is `ls` now that done tasks never leave todo.txt.
    assert_eq!(run(dir.path(), &["lsa"]).1, run(dir.path(), &["ls"]).1);
    assert_eq!(run(dir.path(), &["lsprj"]).1, "+home\n+work\n");
    assert_eq!(run(dir.path(), &["lsc", "beta"]).1, "@desk\n");
    assert_eq!(
        run(dir.path(), &["lf"]).1,
        "Files in the todo.txt directory:\ntodo.txt\n"
    );
    assert!(!run(dir.path(), &["lf", "nope"]).0);
}

#[test]
fn json_lists_one_object_per_line_with_fields_and_spans() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), TODO).unwrap();
    let (ok, out) = run(dir.path(), &["--json", "ls", "+work"]);
    assert!(ok);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[0].starts_with(
        r#"{"line":5,"raw":"(A) urgent +work id:01ARZ3NDEKTSV4RRFFQ69G5FAV","task":true,"#
    ));
    assert!(
        lines[0].contains(r#""id":"01ARZ3NDEKTSV4RRFFQ69G5FAV""#)
            && lines[0].contains(r#""spans":[{"kind":"Priority","start":0,"end":3}"#),
        "{}",
        lines[0]
    );
    assert!(lines[1].contains(r#""contexts":["desk"]"#));
    assert_eq!(
        run(dir.path(), &["--json", "lsprj"]).1,
        "[\"+home\",\"+work\"]\n"
    );
}
