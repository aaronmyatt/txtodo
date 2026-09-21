//! Workspace discovery (plan §3.2.11): every `todo.txt` and `notes.md` under the
//! root, at any depth. Walks the tree, never follows `ref:` tags, so a hand-made directory is
//! found too. Iterative with an explicit stack — no recursion (constitution §3) and no walkdir
//! dependency. https://doc.rust-lang.org/std/fs/fn.read_dir.html
//!
//! `notes.md` is discovered here like any other document (rule 11: "Discovery is by walking the
//! tree, not by following tags") so sync and a `notes.md` Loro-text-doc actor (tasks/crdt-notes-doc)
//! can find it — but it is prose, not a task list, so `Workspace::register` deliberately does not
//! build a line-oriented `FileActor`/`DocState` for it (that stamped `id:` tags into prose, the
//! bug `workspace_tests::notes_md_is_left_alone` guards against). `is_notes_document` is the seam
//! callers use to tell the two kinds of discovered document apart.

use std::fmt;
use std::path::{Path, PathBuf};
use txtodo_model::FilePath;

/// The basenames the walker discovers under a workspace: task documents plus `notes.md`.
pub const DOCUMENT_NAMES: [&str; 2] = ["todo.txt", "notes.md"];
/// `notes.md` specifically — a synced document (plan §3.2 rule 11) that is Loro-text prose, not
/// task lines. See the module doc for why this is discovered but not actor-registered.
pub const NOTES_DOCUMENT_NAME: &str = "notes.md";
/// Deepest directory nesting visited; far above sane `ref:` nesting, so hitting it is an error.
pub const WALK_MAX_DEPTH: usize = 32;
/// Most documents one workspace may hold (plan §5 talks about 10k lines, not 10k files).
pub const WALK_MAX_FILES: usize = 10_000;
/// The daemon's own state directory, never a document.
pub const STATE_DIR: &str = ".txtodo";
/// Directory names the walker never enters wherever they appear: version-control internals and
/// installed dependencies, which hold thousands of files and never a todo list.
const SKIPPED_DIR_NAMES: [&str; 2] = [".git", "node_modules"];

/// Why a walk stopped.
#[derive(Debug)]
pub enum WalkError {
    /// A directory could not be listed.
    Io {
        /// The directory.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// Nesting past `WALK_MAX_DEPTH`.
    TooDeep(PathBuf),
    /// More than `WALK_MAX_FILES` documents.
    TooMany(usize),
    /// A path under the root did not form a valid `FilePath` (non-UTF-8 name).
    BadName(PathBuf),
}

impl fmt::Display for WalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WalkError::Io { path, source } => write!(f, "cannot list {}: {source}", path.display()),
            WalkError::TooDeep(p) => {
                write!(f, "{} is nested deeper than {WALK_MAX_DEPTH}", p.display())
            }
            WalkError::TooMany(n) => write!(f, "{n} documents, max {WALK_MAX_FILES}"),
            WalkError::BadName(p) => write!(f, "{} is not a valid workspace path", p.display()),
        }
    }
}

impl std::error::Error for WalkError {}

/// True for a basename the walker discovers (task documents and `notes.md`).
pub fn is_document_name(name: &str) -> bool {
    DOCUMENT_NAMES.contains(&name)
}

/// True for `notes.md` specifically — see the module doc.
pub fn is_notes_document(name: &str) -> bool {
    name == NOTES_DOCUMENT_NAME
}

/// Every document under `root`, as workspace-relative paths, sorted. `root` itself is depth 0. A
/// thin span wrapper around `walk_inner` (`#[instrument]` on the real body overflows).
#[tracing::instrument(skip_all, fields(root = %root.display()))]
pub fn walk(root: &Path) -> Result<Vec<FilePath>, WalkError> {
    walk_inner(root, None)
}

/// `walk`, also finding `extra`: an absolute path of a document whose name the walker would not
/// find on its own, the workspace's root list when `todo_file` names one (task workspace-layout).
pub fn walk_with(root: &Path, extra: Option<&Path>) -> Result<Vec<FilePath>, WalkError> {
    walk_inner(root, extra)
}

fn walk_inner(root: &Path, extra: Option<&Path>) -> Result<Vec<FilePath>, WalkError> {
    let mut found = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    // Bounded: every directory is pushed once and popped once; the file cap bounds `found`.
    while let Some((dir, depth)) = stack.pop() {
        if depth > WALK_MAX_DEPTH {
            return Err(WalkError::TooDeep(dir));
        }
        let entries = std::fs::read_dir(&dir).map_err(|source| WalkError::Io {
            path: dir.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| WalkError::Io {
                path: dir.clone(),
                source,
            })?;
            visit((root, extra), &entry, depth, &mut stack, &mut found)?;
        }
    }
    found.sort();
    debug_assert!(found.len() <= WALK_MAX_FILES);
    debug_assert!(found.windows(2).all(|w| w[0] < w[1]), "sorted and unique");
    log_walk_complete(found.len());
    Ok(found)
}

/// Split out so the event macro doesn't count against `walk`'s own `#[instrument]` budget.
fn log_walk_complete(found: usize) {
    tracing::debug!(found, "walk_complete");
}

fn visit(
    (root, extra): (&Path, Option<&Path>),
    entry: &std::fs::DirEntry,
    depth: usize,
    stack: &mut Vec<(PathBuf, usize)>,
    found: &mut Vec<FilePath>,
) -> Result<(), WalkError> {
    let path = entry.path();
    let name = entry.file_name();
    let Some(name) = name.to_str() else {
        return Err(WalkError::BadName(path));
    };
    // symlink_metadata does not follow links: a symlinked directory is neither entered nor listed.
    let meta = std::fs::symlink_metadata(&path).map_err(|source| WalkError::Io {
        path: path.clone(),
        source,
    })?;
    if meta.is_dir() {
        if name != STATE_DIR && !is_skipped_dir(&path) {
            stack.push((path, depth + 1));
        }
        return Ok(());
    }
    if meta.is_file() && (is_document_name(name) || extra == Some(path.as_path())) {
        if found.len() == WALK_MAX_FILES {
            return Err(WalkError::TooMany(found.len() + 1));
        }
        found.push(relative(root, &path)?);
    }
    Ok(())
}

/// True for a directory that is never part of a workspace's documents: `.git` and `node_modules`;
/// a Cargo build directory (`target` holding the `CACHEDIR.TAG` cargo writes into it, or sitting
/// beside a `Cargo.toml`); and `.claude/worktrees`, whose checkouts are whole copies of the repo
/// (registered as workspaces of their own when they matter, never a subtree of this one). Walking
/// them made the daemon adopt about 2979 documents against 361 real ones and re-walk every new
/// directory a `cargo build` created under `target/` (root todo id:01M2WK7W1MPDW9VBWS25EF8CB5).
/// A ref directory that merely happens to be called `target` is still walked.
/// Ref: <https://bford.info/cachedir/>
pub fn is_skipped_dir(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if SKIPPED_DIR_NAMES.contains(&name) {
        return true;
    }
    let parent = dir.parent();
    match name {
        "target" => {
            dir.join("CACHEDIR.TAG").is_file()
                || parent.is_some_and(|p| p.join("Cargo.toml").is_file())
        }
        "worktrees" => parent.and_then(Path::file_name).and_then(|n| n.to_str()) == Some(".claude"),
        _ => false,
    }
}

/// True when `path` (a directory or a document) is at or below a skipped directory that lies
/// *inside* `root` — the watcher's routing test, since `notify` reports every file a build writes.
/// `root`'s own ancestors do not count: a workspace registered at `.../.claude/worktrees/x` is a
/// real workspace.
pub fn is_in_skipped_dir(root: &Path, path: &Path) -> bool {
    let Ok(below) = path.strip_prefix(root) else {
        return false;
    };
    let mut current = root.to_path_buf();
    // Bounded by the path's own component count.
    below.components().any(|part| {
        current.push(part);
        is_skipped_dir(&current)
    })
}

/// `root/a/b/todo.txt` → `a/b/todo.txt` with `/` separators on every platform.
pub fn relative(root: &Path, path: &Path) -> Result<FilePath, WalkError> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| WalkError::BadName(path.to_path_buf()))?;
    let mut parts = Vec::new();
    for c in rel.components() {
        let Some(s) = c.as_os_str().to_str() else {
            return Err(WalkError::BadName(path.to_path_buf()));
        };
        parts.push(s);
    }
    debug_assert!(
        !parts.is_empty(),
        "a document path has at least a file name"
    );
    FilePath::new(&parts.join("/")).map_err(|_| WalkError::BadName(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap_or_else(|e| panic!("{e}"));
        }
        std::fs::write(p, b"").unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn finds_documents_at_any_depth_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let r = dir.path();
        for p in [
            "todo.txt",
            "q4/todo.txt",
            "q4/notes.md",
            "q4/sync/todo.txt",
            "q4/other.txt",
            ".txtodo/oplog.db",
            "notes.txt",
        ] {
            touch(&r.join(p));
        }
        touch(&r.join(".hidden").join("todo.txt"));
        let found: Vec<String> = walk(r)
            .unwrap_or_else(|e| panic!("{e}"))
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            found,
            vec![
                ".hidden/todo.txt",
                "q4/notes.md",
                "q4/sync/todo.txt",
                "q4/todo.txt",
                "todo.txt"
            ]
        );
        assert!(
            is_document_name("notes.md") && is_notes_document("notes.md"),
            "notes.md is a synced document (plan §3.2 rule 11)"
        );
    }

    #[test]
    fn skips_vcs_dependency_build_and_worktree_trees_but_not_lookalikes() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let r = dir.path();
        for p in [
            "todo.txt",
            // Skipped: version control, dependencies, the two cargo-target markers, worktrees.
            ".git/todo.txt",
            "web/node_modules/pkg/todo.txt",
            "sub/target/CACHEDIR.TAG",
            "sub/target/debug/todo.txt",
            "app/Cargo.toml",
            "app/target/todo.txt",
            ".claude/worktrees/wt/todo.txt",
            // Kept: a ref dir that is only called `target`, `worktrees` outside `.claude`, and the
            // rest of `.claude`.
            "tasks/target/todo.txt",
            "worktrees/todo.txt",
            ".claude/todo.txt",
        ] {
            touch(&r.join(p));
        }
        let found: Vec<String> = walk(r)
            .unwrap_or_else(|e| panic!("{e}"))
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            found,
            vec![
                ".claude/todo.txt",
                "tasks/target/todo.txt",
                "todo.txt",
                "worktrees/todo.txt"
            ]
        );
    }

    #[test]
    fn is_in_skipped_dir_looks_only_below_the_root() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let r = dir.path();
        touch(&r.join("Cargo.toml"));
        std::fs::create_dir_all(r.join("target/debug/deps")).unwrap_or_else(|e| panic!("{e}"));
        assert!(is_in_skipped_dir(r, &r.join("target/debug/deps")));
        assert!(is_in_skipped_dir(r, &r.join("target/debug/todo.txt")));
        assert!(is_in_skipped_dir(r, &r.join(".git/hooks")));
        assert!(!is_in_skipped_dir(r, &r.join("q4/sub")));
        assert!(!is_in_skipped_dir(r, r), "the root itself is never skipped");
        // A workspace that itself lives in a worktree directory is a real workspace.
        let inside = r.join(".claude/worktrees/wt");
        std::fs::create_dir_all(inside.join("q4")).unwrap_or_else(|e| panic!("{e}"));
        assert!(!is_in_skipped_dir(&inside, &inside.join("q4")));
        assert!(is_in_skipped_dir(r, &inside.join("q4")));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_loop_terminates() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let r = dir.path();
        touch(&r.join("a").join("todo.txt"));
        std::os::unix::fs::symlink(r.join("a"), r.join("a").join("loop"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(walk(r).unwrap_or_else(|e| panic!("{e}")).len(), 1);
        assert!(is_document_name("todo.txt") && !is_document_name("todo.cfg"));
    }

    #[test]
    fn walk_with_finds_the_named_extra_document_and_nothing_else_new() {
        let r = tempfile::tempdir().unwrap();
        let r = r.path();
        touch(&r.join("lists/work.txt"));
        touch(&r.join("lists/other.txt"));
        touch(&r.join("todo.txt"));
        let names = |extra: Option<&Path>| -> Vec<String> {
            walk_with(r, extra)
                .unwrap()
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(names(None), ["todo.txt"], "an unknown name is invisible");
        assert_eq!(
            names(Some(&r.join("lists/work.txt"))),
            ["lists/work.txt", "todo.txt"]
        );
    }
}
