//! Unit-test-only switch between the real file watcher and an inert one.
//!
//! A unit test that opens a workspace through `open_workspace_full` used to start a real `notify`
//! watcher each time. On macOS that is an FSEvents stream, and many test processes registering
//! streams at once queue behind `fseventsd`: median 20-25 s per test, up to 300 s, for tests that
//! never look at a file event. So unit tests get `WatchGuard::Inert` by default, and a test that
//! needs file events calls [`use_real_watcher`] on its directory before the open. Only compiled
//! under `#[cfg(test)]` (`lib.rs`): `tests/*.rs` and every txtodod build always watch for real.
//! https://doc.rust-lang.org/reference/conditional-compilation.html#test
//!
//! On the real watcher today: `workspace_catalog_tests::an_outside_edit_reaches_the_open_
//! workspace_through_the_real_watcher` (events flow) and `workspace_rejoin_tests::a_rejoin_moves_
//! the_copy_aside_…` (files moved under a live watch). Each costs 40-70 s on a busy Mac, all of it
//! in `FSEventStreamStart` waiting on `fseventsd`: opt in only a test that needs file events.
//!
//! Keyed by path, not by thread: an open usually runs on tokio's blocking pool or a worker thread,
//! where a `thread_local!` set on the test's own thread would never be seen. Test tempdirs are
//! unique, so under `cargo test` (every test in one process) one test's opt-in never reaches
//! another test's workspace.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// Directories whose workspaces (at or below) get the real watcher. `Mutex::new` is `const`, so a
/// plain `static` needs no lazy init. https://doc.rust-lang.org/std/sync/struct.Mutex.html#method.new
static REAL: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Every workspace opened at or below `dir` from now on gets the real `notify` watcher.
pub(crate) fn use_real_watcher(dir: &Path) {
    REAL.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(canonical(dir));
}

/// True when `root` sits at or below a directory passed to [`use_real_watcher`].
pub(crate) fn wants_real_watcher(root: &Path) -> bool {
    let root = canonical(root);
    REAL.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .any(|dir| root.starts_with(dir))
}

/// A macOS tempdir under `/var` is really under `/private/var`: compare resolved paths.
/// https://doc.rust-lang.org/std/fs/fn.canonicalize.html
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_workspace_at_or_below_an_opted_in_dir_gets_the_real_watcher() {
        let (opted, other) = (tempfile::tempdir(), tempfile::tempdir());
        let (opted, other) = (
            opted.unwrap_or_else(|e| panic!("{e}")),
            other.unwrap_or_else(|e| panic!("{e}")),
        );
        let below = opted.path().join("sub");
        std::fs::create_dir(&below).unwrap_or_else(|e| panic!("{e}"));
        assert!(!wants_real_watcher(opted.path()), "inert by default");

        use_real_watcher(opted.path());
        assert!(wants_real_watcher(opted.path()));
        assert!(wants_real_watcher(&below), "a workspace below the dir");
        assert!(!wants_real_watcher(other.path()), "another test's dir");
    }
}
