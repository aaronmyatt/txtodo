//! The file watcher (plan M3): `notify` events → ignore rules → per-path debounce → the actor's
//! `ExternalChange`, or a directory-create → `Workspace::discover`. Only routing lives here; the
//! debounce is pure (`debounce.rs`) and driven by the injected clock. https://docs.rs/notify

use crate::debounce::Debouncer;
use crate::walker::{STATE_DIR, is_document_name};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::sync::mpsc;

/// Editor droppings that never belong to a document. Basename globs.
pub const IGNORE_GLOBS: [&str; 4] = ["*.swp", "*~", "*.tmp", ".#*"];
/// Raw events buffered between the notify thread and the async drain.
pub const RAW_EVENT_CAP: usize = 4096;

/// What the drain hands on after ignore rules and debounce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// A document changed (absolute path).
    Document(PathBuf),
    /// A directory appeared (absolute path); the workspace should walk it.
    Directory(PathBuf),
}

/// A raw event reduced to what routing needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvent {
    /// Absolute path.
    pub path: PathBuf,
    /// A directory-create event, as opposed to any file event.
    pub dir_created: bool,
}

/// True when the basename matches an ignore glob or the path is under `.txtodo`.
pub fn is_ignored(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    if path.components().any(|c| c.as_os_str() == STATE_DIR) || name.starts_with(".txtodo-") {
        return true;
    }
    IGNORE_GLOBS.iter().any(|g| glob_match(g, name))
}

/// `*` at either end only — enough for the four patterns above, no dependency.
fn glob_match(glob: &str, name: &str) -> bool {
    debug_assert!(glob.matches('*').count() <= 1, "one star per pattern");
    match (glob.strip_prefix('*'), glob.strip_suffix('*')) {
        (Some(suffix), _) => name.ends_with(suffix) && name.len() > suffix.len(),
        (None, Some(prefix)) => name.starts_with(prefix) && name.len() > prefix.len(),
        (None, None) => glob == name,
    }
}

/// Feeds one raw event into the debouncer or routes a directory at once. Pure. A thin span
/// wrapper around `ingest_inner`; the two events live in their own tiny functions since an
/// inline `tracing::debug!` here pushes `#[instrument]`'s own expansion over budget.
#[tracing::instrument(skip_all, fields(dir_created = ev.dir_created))]
pub fn ingest(
    deb: &mut Debouncer,
    ev: RawEvent,
    now: Instant,
    extra: Option<&Path>,
) -> Option<Routed> {
    ingest_inner(deb, ev, now, extra)
}

/// `extra` is the root list's absolute path when its name is not a document name (`todo_file`).
fn ingest_inner(
    deb: &mut Debouncer,
    ev: RawEvent,
    now: Instant,
    extra: Option<&Path>,
) -> Option<Routed> {
    if ev.dir_created {
        let routed = (!is_ignored(&ev.path)).then_some(Routed::Directory(ev.path));
        log_directory_event(routed.is_some());
        return routed;
    }
    let name = ev
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let is_layout = name == crate::layout_file::LAYOUT_FILE;
    let is_extra = extra == Some(ev.path.as_path());
    if is_ignored(&ev.path) || !(is_document_name(name) || is_layout || is_extra) {
        return None;
    }
    // A dropped event on overflow is a flood, not a save; the next event for the path repairs it.
    let accepted = deb.push(ev.path, now);
    log_document_event(accepted, deb.pending());
    None
}

fn log_directory_event(routed: bool) {
    tracing::debug!(routed, "directory_event");
}

fn log_document_event(accepted: bool, pending: usize) {
    tracing::debug!(accepted, pending, "document_event_debounced");
}

/// What keeps a workspace's watch alive. The caller holds it; dropping it stops events.
pub enum WatchGuard {
    /// The real `notify` watcher on the workspace root (FSEvents on macOS, inotify on Linux).
    /// https://docs.rs/notify/8/notify/type.RecommendedWatcher.html
    Notify(RecommendedWatcher),
    /// Unit-test builds only, the default there (`crate::watch_opt_in`): no OS watch at all, just
    /// the raw-event sender held open so the drain idles instead of ending. `cfg(test)` is set
    /// only for this crate's own unit-test harness, never for `tests/*.rs` or a txtodod build, so
    /// a shipped binary cannot hold this variant.
    /// https://doc.rust-lang.org/reference/conditional-compilation.html#test
    #[cfg(test)]
    Inert(mpsc::Sender<RawEvent>),
}

/// Starts `notify` on `root` (recursive). Events arrive on the returned receiver as `RawEvent`s;
/// the guard must be kept alive by the caller (dropping it stops events).
pub fn start(root: &Path) -> notify::Result<(WatchGuard, mpsc::Receiver<RawEvent>)> {
    let (tx, rx) = mpsc::channel(RAW_EVENT_CAP);
    // A unit test that never looks at a file event skips the OS watch: registering one FSEvents
    // stream per test process is what made catalog tests take 20 s to 300 s each.
    #[cfg(test)]
    if !crate::watch_opt_in::wants_real_watcher(root) {
        return Ok((WatchGuard::Inert(tx), rx));
    }
    Ok((WatchGuard::Notify(notify_watch(root, tx)?), rx))
}

/// The real OS watch on `root`, feeding `tx`.
fn notify_watch(root: &Path, tx: mpsc::Sender<RawEvent>) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        // `Access` (open/read/close-no-write) never signals a content change; inotify emits it for
        // a plain read, and on some backends/filesystems it can repeat indefinitely for a file that
        // is merely open. Forwarding it would let it re-arm the debounce forever, so the file that
        // triggered it can never settle — filtered here rather than in `ingest`, so the debounce
        // never even learns of an event with no signal in it.
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let dir_created = matches!(
            event.kind,
            EventKind::Create(notify::event::CreateKind::Folder)
        );
        for path in event.paths {
            // Backpressure: a full buffer drops the event; the debounce collapses bursts anyway.
            let _sent = tx.try_send(RawEvent { path, dir_created });
        }
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debounce::DEBOUNCE_MS;
    use std::time::Duration;

    #[test]
    fn ignore_rules_cover_editor_droppings_and_the_state_dir() {
        for p in [
            ".todo.txt.swp",
            "todo.txt~",
            "x.tmp",
            ".#todo.txt",
            ".txtodo/oplog.db",
            ".txtodo-todo.txt.1.tmp",
        ] {
            assert!(is_ignored(Path::new(p)), "{p}");
        }
        for p in ["todo.txt", "q4/other.txt", "notes.md", "swp", "~"] {
            assert!(!is_ignored(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn ingest_debounces_documents_and_routes_directories_immediately() {
        let t0 = Instant::now();
        let mut deb = Debouncer::default();
        let ev = |p: &str, d: bool| RawEvent {
            path: PathBuf::from(p),
            dir_created: d,
        };
        assert_eq!(
            ingest(&mut deb, ev("/w/q4", true), t0, None),
            Some(Routed::Directory(PathBuf::from("/w/q4")))
        );
        assert_eq!(ingest(&mut deb, ev("/w/.txtodo", true), t0, None), None);
        for i in 0..5u64 {
            assert_eq!(
                ingest(
                    &mut deb,
                    ev("/w/todo.txt", false),
                    t0 + Duration::from_millis(i * 10),
                    None
                ),
                None
            );
        }
        assert_eq!(ingest(&mut deb, ev("/w/other.txt", false), t0, None), None);
        assert_eq!(
            ingest(&mut deb, ev("/w/.todo.txt.swp", false), t0, None),
            None
        );
        assert_eq!(deb.pending(), 1);
        assert!(
            deb.drain_due(t0 + Duration::from_millis(40 + DEBOUNCE_MS - 1))
                .is_empty()
        );
        assert_eq!(
            deb.drain_due(t0 + Duration::from_millis(40 + DEBOUNCE_MS)),
            vec![PathBuf::from("/w/todo.txt")]
        );
    }

    #[test]
    fn the_layout_file_is_debounced_like_a_document_and_other_toml_is_not() {
        let t0 = Instant::now();
        let mut deb = Debouncer::default();
        let ev = |p: &str| RawEvent {
            path: PathBuf::from(p),
            dir_created: false,
        };
        assert_eq!(ingest(&mut deb, ev("/w/other.toml"), t0, None), None);
        assert_eq!(
            deb.pending(),
            0,
            "an unrelated toml never reaches the debounce"
        );
        assert_eq!(ingest(&mut deb, ev("/w/txtodo.toml"), t0, None), None);
        assert_eq!(deb.pending(), 1, "the layout file does");
    }

    #[test]
    fn a_custom_root_list_is_debounced_only_when_the_workspace_names_it() {
        let t0 = Instant::now();
        let mut deb = Debouncer::default();
        let ev = RawEvent {
            path: PathBuf::from("/w/work.txt"),
            dir_created: false,
        };
        let extra = PathBuf::from("/w/work.txt");
        assert_eq!(ingest(&mut deb, ev.clone(), t0, None), None);
        assert_eq!(deb.pending(), 0, "an unknown name is ignored");
        assert_eq!(ingest(&mut deb, ev, t0, Some(&extra)), None);
        assert_eq!(deb.pending(), 1, "the named root list is watched");
    }
}
