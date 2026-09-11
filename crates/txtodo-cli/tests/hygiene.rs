//! File hygiene the CLI must keep (design §2.2 rule 7, plan M2 acceptance): `add` on a CRLF file
//! keeps CRLF; on a file without a trailing newline it terminates the last line first, like todo.sh.
//! Plus the one place txtodo deliberately differs from todo.sh: `do` keeps the priority as `pri:`.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

fn txtodo(dir: &Path, args: &[&str]) -> bool {
    Command::new(env!("CARGO_BIN_EXE_txtodo"))
        .current_dir(dir)
        .env_remove("TXTODO_TODO_DIR")
        .env("TXTODO_CONFIG", dir.join("none.toml"))
        .arg("--no-id")
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("txtodo runs: {e}"))
        .success()
}

fn todo_bytes(dir: &Path) -> Vec<u8> {
    std::fs::read(dir.join("todo.txt")).unwrap()
}

fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

#[test]
fn add_on_a_crlf_file_keeps_crlf_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), b"first\r\nsecond\r\n").unwrap();
    assert!(txtodo(dir.path(), &["add", "third"]));
    assert_eq!(
        todo_bytes(dir.path()),
        format!("first\r\nsecond\r\n{} third\r\n", today()).into_bytes()
    );
    assert!(txtodo(dir.path(), &["-A", "do", "2"]));
    assert_eq!(
        todo_bytes(dir.path()),
        format!("first\r\nx {d} second\r\n{d} third\r\n", d = today()).into_bytes()
    );
}

#[test]
fn add_on_a_file_without_trailing_newline_terminates_it_first_like_todo_sh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("todo.txt"), b"first\nsecond").unwrap();
    assert!(txtodo(dir.path(), &["add", "third"]));
    assert_eq!(
        todo_bytes(dir.path()),
        format!("first\nsecond\n{} third\n", today()).into_bytes()
    );
    std::fs::write(dir.path().join("todo.txt"), b"only\r\nlast").unwrap();
    assert!(txtodo(dir.path(), &["add", "next"]));
    assert_eq!(
        todo_bytes(dir.path()),
        format!("only\r\nlast\r\n{} next\r\n", today()).into_bytes()
    );
    let bom = dir.path().join("todo.txt");
    std::fs::write(&bom, b"\xEF\xBB\xBFone\n").unwrap();
    assert!(txtodo(dir.path(), &["pri", "1", "a"]));
    assert_eq!(todo_bytes(dir.path()), b"\xEF\xBB\xBF(A) one\n");
}

#[test]
fn do_keeps_the_priority_as_a_pri_tag_unlike_todo_sh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("todo.txt"),
        "(B) 2026-09-01 urgent +work\nplain\n",
    )
    .unwrap();
    assert!(txtodo(dir.path(), &["do", "1"]));
    assert_eq!(todo_bytes(dir.path()), b"plain\n");
    let done = std::fs::read_to_string(dir.path().join("done.txt")).unwrap();
    assert_eq!(
        done,
        format!("x {} 2026-09-01 urgent +work pri:B\n", today())
    );
}

#[test]
fn add_creates_a_missing_todo_dir_like_todo_sh() {
    let dir = tempfile::tempdir().unwrap();
    let new = dir.path().join("new").join("sub");
    assert!(!new.exists());
    assert!(txtodo(
        dir.path(),
        &["--dir", new.to_str().unwrap(), "add", "call mum"]
    ));

    assert_eq!(
        todo_bytes(&new),
        format!("{} call mum\n", today()).into_bytes()
    );
}
