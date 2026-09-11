# Watcher with notify, 150 ms debounce, ignore list *.swp *~ *.tmp .#* (plan M3)

Design §4.3 step 1: watcher fires, debounced 150 ms, ignoring editor temp files and partial writes.
Ref: https://docs.rs/notify · tokio paused time https://docs.rs/tokio/latest/tokio/time/fn.pause.html

## Pipeline
`notify` callback (its own thread) → `std::sync::mpsc`/`tokio::sync::mpsc::Sender::blocking_send`
→ async drain task → `Ignore::keep(path)` → `Debouncer::push(path, now)` → on expiry
`ActorHandle::send(ExternalChange)` or `Walker::discover(dir)`.

## Debouncer (pure, testable without a filesystem)
```rust
pub struct Debouncer { pending: BTreeMap<PathBuf, Instant>, window_ms: u64 }
impl Debouncer {
    pub fn push(&mut self, path: PathBuf, now: Instant);            // resets that path's deadline
    pub fn drain_due(&mut self, now: Instant) -> Vec<PathBuf>;      // paths whose deadline passed
    pub fn next_deadline(&self) -> Option<Instant>;                 // what the task sleeps until
}
```
`pending.len() <= WATCH_EVENT_CAP` is asserted; the map is the only buffer and it is bounded.

## Ignore rules
Glob match on the basename only. `*~` covers Emacs/vim backups, `.#*` Emacs lock symlinks, `*.swp`
vim swap, `*.tmp` generic. The daemon's own temp file (`.todo.txt.txtodo-tmp`) is matched by the
`.txtodo` prefix rule so our rename never re-triggers via the temp name (the target name does
fire, and tasks/daemon-own-writes handles that).

## Platform notes
FSEvents coalesces; inotify may deliver `Modify` twice per save. Both are why the debounce plus
the hash check exist. Windows keeps the same code path; named pipes are elsewhere (proto).
