//! `txtodo lists` and `--list NAME` end to end (task `two-kinds-of-workspace`, ADR 0043): a real
//! `txtodod` in true global mode, hermetic via `$TXTODO_SOCKET`, so its lists live in
//! `<state dir>/lists/`. Every call runs from a folder that is no workspace, which proves a list
//! is named, not found by where you stand.
// Integration tests are tests: clippy.toml allows unwrap/expect in #[test] fns but not in their helpers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use crate::workspace::{GlobalDaemon, stdout, txtodo};

use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Waits for `file` to hold `needle`: the daemon writes a moment after it answers.
fn wait_for(file: &Path, needle: &str) -> String {
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{} never held {needle:?}; it holds {text:?}",
            file.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A daemon, the folder every call runs from (no workspace), and its state dir.
pub(crate) struct Fixture {
    pub(crate) state: tempfile::TempDir,
    nowhere: tempfile::TempDir,
    daemon: GlobalDaemon,
}

impl Fixture {
    pub(crate) fn new() -> Fixture {
        let state = tempfile::tempdir().unwrap();
        let daemon = GlobalDaemon::spawn(state.path());
        Fixture {
            state,
            nowhere: tempfile::tempdir().unwrap(),
            daemon,
        }
    }

    pub(crate) fn run(&self, args: &[&str]) -> Output {
        txtodo(&self.daemon, self.nowhere.path(), args)
    }

    fn list_file(&self, name: &str) -> std::path::PathBuf {
        self.state.path().join("lists").join(name).join("todo.txt")
    }
}

#[test]
fn a_list_is_made_and_worked_on_by_name_from_anywhere() {
    let f = Fixture::new();
    let made = f.run(&["lists", "new", "Work"]);
    assert!(made.status.success(), "{}", stderr(&made));
    assert!(
        stdout(&made).contains("made list work"),
        "{}",
        stdout(&made)
    );
    assert!(f.list_file("work").is_file());
    assert!(stdout(&f.run(&["lists", "new", "work"])).contains("already here"));

    let added = f.run(&["--list", "work", "add", "buy milk"]);
    assert!(added.status.success(), "{}", stderr(&added));
    wait_for(&f.list_file("work"), "buy milk");
    assert!(stdout(&f.run(&["--list", "work", "ls"])).contains("buy milk"));
}

#[test]
fn lists_shows_every_list_and_a_name_that_is_no_list_says_how_to_make_one() {
    let f = Fixture::new();
    f.run(&["lists", "new", "work"]);
    let shown = stdout(&f.run(&["lists"]));
    assert!(
        shown.contains("work  ") && shown.contains("[default]"),
        "{shown}"
    );
    let json = stdout(&f.run(&["--json", "lists", "ls"]));
    assert!(json.contains(r#""name":"work""#), "{json}");
    assert!(json.contains(r#""is_default":true"#), "{json}");

    let missing = f.run(&["--list", "nope", "ls"]);
    assert!(!missing.status.success());
    assert!(
        stderr(&missing).contains("lists new nope"),
        "{}",
        stderr(&missing)
    );
}

#[test]
fn a_rename_changes_what_a_list_shows_and_the_made_under_name_still_finds_it() {
    let f = Fixture::new();
    f.run(&["lists", "new", "work"]);
    f.run(&["--list", "work", "add", "buy milk"]);
    wait_for(&f.list_file("work"), "buy milk");

    let renamed = f.run(&["lists", "rename", "work", "projects"]);
    assert!(renamed.status.success(), "{}", stderr(&renamed));
    for name in ["projects", "work"] {
        let by = f.run(&["--list", name, "ls"]);
        assert!(stdout(&by).contains("buy milk"), "{name}: {}", stderr(&by));
    }
    assert!(stdout(&f.run(&["lists"])).contains("projects  "));
}

#[test]
fn a_removed_list_moves_aside_with_its_text_and_the_default_stays() {
    let f = Fixture::new();
    f.run(&["lists", "new", "work"]);
    f.run(&["--list", "work", "add", "buy milk"]);
    wait_for(&f.list_file("work"), "buy milk");

    assert!(!f.run(&["lists", "rm", "default"]).status.success());
    let gone = f.run(&["lists", "rm", "work"]);
    assert!(gone.status.success(), "{}", stderr(&gone));
    assert!(
        stdout(&gone).contains("your other devices keep it"),
        "{}",
        stdout(&gone)
    );
    assert!(!stdout(&f.run(&["lists"])).contains("work  "));
    let moved: Vec<_> = std::fs::read_dir(f.state.path().join("lists"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("work."))
        .collect();
    assert_eq!(moved.len(), 1, "one folder moved aside");
    let kept = std::fs::read_to_string(moved[0].path().join("todo.txt")).unwrap();
    assert!(kept.contains("buy milk"), "{kept}");
}

#[test]
fn a_folder_is_imported_into_a_new_list() {
    let f = Fixture::new();
    let run = |args: &[&str]| f.run(args);

    let src = tempfile::Builder::new()
        .prefix("Old Notes")
        .tempdir()
        .unwrap();
    std::fs::write(src.path().join("todo.txt"), "from the old folder\n").unwrap();
    std::fs::create_dir_all(src.path().join("tasks/trip")).unwrap();
    std::fs::write(src.path().join("tasks/trip/todo.txt"), "book flights\n").unwrap();
    std::fs::create_dir_all(src.path().join(".git")).unwrap();

    let src_arg = src.path().to_str().unwrap();
    let imported = run(&["lists", "import", src_arg, "old"]);
    assert!(imported.status.success(), "{}", stderr(&imported));
    let list = f.state.path().join("lists/old");
    wait_for(&list.join("todo.txt"), "from the old folder");
    assert_eq!(
        std::fs::read_to_string(list.join("tasks/trip/todo.txt")).unwrap(),
        "book flights\n"
    );
    assert!(!list.join(".git").exists(), "hidden folders stay behind");

    // Into a list that is there already: refused, nothing mixed in.
    let again = run(&["lists", "import", src_arg, "old"]);
    assert!(!again.status.success());
    assert!(
        stderr(&again).contains("already exists"),
        "{}",
        stderr(&again)
    );
    // A folder with no todo.txt has nothing to import.
    let empty = tempfile::tempdir().unwrap();
    let none = run(&["lists", "import", empty.path().to_str().unwrap(), "x"]);
    assert!(stderr(&none).contains("no todo.txt"), "{}", stderr(&none));
}
