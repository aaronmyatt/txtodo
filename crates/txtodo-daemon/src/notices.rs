//! The daemon's own change notices (ADR 0041, task daemon-notices): ops made on another device
//! (commit source `sync`), folded per burst by `txtodo_core::notice::Debouncer::only_from("sync")`,
//! shown as one OS notice unless a client that shows its own is connected
//! (`WatchRequest.shows_notices`), `txtodo notices off` left its marker, or the notifier failed
//! once. Device-wide: one hub for every workspace, installed by `main` for the global daemon only
//! and not with `TXTODO_NO_NOTICES` set; tests and the in-process harnesses have none.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use txtodo_core::notice::{Debouncer, Notice, OpFacts};

use crate::commit::SYNC_SOURCE;
use crate::handle::Change;

/// The marker `txtodo notices off` leaves in the daemon's state dir; `on` removes it.
pub const OFF_MARKER: &str = "notices-off";

/// Shows one notice. A trait so tests record instead of showing.
pub trait Notifier: Send + Sync {
    /// Shows `title` and `body`; `Err` with why it could not.
    fn show(&self, title: &str, body: &str) -> Result<(), String>;
}

/// The OS notice, by shelling out with the text as separate argv items, never spliced into a
/// script (ADR 0041): macOS `osascript`, Linux `notify-send`, elsewhere nothing.
/// Ref: <https://developer.apple.com/library/archive/documentation/LanguagesUtilities/Conceptual/MacAutomationScriptingGuide/DisplayNotifications.html>
pub struct OsNotifier;

impl OsNotifier {
    /// The program and arguments that show `title` and `body` on this OS, if any.
    pub fn command(title: &str, body: &str) -> Option<(&'static str, Vec<String>)> {
        if cfg!(target_os = "macos") {
            let script = [
                "on run argv",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "end run",
            ];
            let mut args: Vec<String> = script
                .iter()
                .flat_map(|l| ["-e".to_owned(), (*l).to_owned()])
                .collect();
            args.extend([title.to_owned(), body.to_owned()]);
            Some(("osascript", args))
        } else if cfg!(target_os = "linux") {
            Some(("notify-send", vec![title.to_owned(), body.to_owned()]))
        } else {
            None
        }
    }
}

impl Notifier for OsNotifier {
    fn show(&self, title: &str, body: &str) -> Result<(), String> {
        let (program, args) = Self::command(title, body).ok_or("no notifier on this OS")?;
        let status = std::process::Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("{program}: {e}"))?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("{program}: {status}"))
    }
}

/// The device's notice state.
pub struct Hub {
    debouncer: Mutex<Debouncer>,
    owners: AtomicUsize,
    off: AtomicBool,
    armed: AtomicBool,
    notifier: Box<dyn Notifier>,
    state_dir: Option<PathBuf>,
}

/// Holds one notice owner (a Watch stream that set `shows_notices`) until dropped.
pub struct OwnerGuard(Arc<Hub>);

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        self.0.owners.fetch_sub(1, Ordering::SeqCst);
    }
}

static HUB: OnceLock<Arc<Hub>> = OnceLock::new();

/// Installs the device's hub (once, from `main`).
pub fn install(hub: Arc<Hub>) {
    let _ = HUB.set(hub);
}

/// `main`'s install: the OS notifier, for the true global daemon only (never a `--dir` bridge, a
/// test or lab harness) and not with `TXTODO_NO_NOTICES` set; `state_dir` holds the off marker.
pub fn install_os(dir_bridge: bool, state_dir: &std::path::Path) {
    if !dir_bridge && std::env::var_os("TXTODO_NO_NOTICES").is_none() {
        install(Hub::new(
            Box::new(OsNotifier),
            Some(state_dir.to_path_buf()),
        ));
    }
}

/// The installed hub, if any.
pub fn hub() -> Option<&'static Arc<Hub>> {
    HUB.get()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Hub {
    /// A hub showing through `notifier`, its off marker looked for in `state_dir`.
    pub fn new(notifier: Box<dyn Notifier>, state_dir: Option<PathBuf>) -> Arc<Hub> {
        Arc::new(Hub {
            debouncer: Mutex::new(Debouncer::only_from(SYNC_SOURCE)),
            owners: AtomicUsize::new(0),
            off: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            notifier,
            state_dir,
        })
    }

    /// One more client showing its own notices, until the guard drops.
    pub fn owner(self: &Arc<Self>) -> OwnerGuard {
        self.owners.fetch_add(1, Ordering::SeqCst);
        OwnerGuard(self.clone())
    }

    /// Feeds a committed change at `now`; true when it counted. Only a sync commit can.
    pub fn take_at(&self, change: &Change, now: u64) -> bool {
        let Some(source) = change.source.as_deref().filter(|s| *s == SYNC_SOURCE) else {
            return false;
        };
        let mut d = self
            .debouncer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut counted = false;
        for stored in &change.ops {
            let op = &stored.op;
            let task = crate::convert::task_of(&op.kind)
                .map(|t| t.to_string())
                .unwrap_or_default();
            let summary = crate::convert::summary_of(&op.kind);
            counted |= d.push(
                now,
                OpFacts {
                    path: change.path.as_str(),
                    source,
                    kind: txtodo_store::kind_tag(&op.kind),
                    task_id: &task,
                    summary: &summary,
                },
            );
        }
        counted
    }

    /// Feeds a committed change and, when it counted and no timer waits, starts one.
    pub fn take(self: &Arc<Self>, change: &Change) {
        if self.take_at(change, now_ms()) && !self.armed.swap(true, Ordering::SeqCst) {
            let hub = self.clone();
            std::thread::spawn(move || hub.wait_and_show());
        }
    }

    /// The notice due at `now`, shown unless something holds it; what was shown.
    pub fn flush_at(&self, now: u64) -> Option<Notice> {
        let notice = self
            .debouncer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .due(now)?;
        if self.held() {
            return None;
        }
        let shown = self.notifier.show(&notice.title, &notice.body);
        shown.map_err(|e| self.turn_off(&e)).ok().map(|()| notice)
    }

    /// A failed show turns notices off until restart, with one warn.
    fn turn_off(&self, why: &str) {
        if !self.off.swap(true, Ordering::SeqCst) {
            tracing::warn!(error = %why, "notice_failed_off_until_restart");
        }
    }

    /// Whether a notice must not show now: a client shows its own, or notices are off.
    fn held(&self) -> bool {
        self.owners.load(Ordering::SeqCst) > 0
            || self.off.load(Ordering::SeqCst)
            || self
                .state_dir
                .as_ref()
                .is_some_and(|d| d.join(OFF_MARKER).exists())
    }

    fn next_due(&self) -> Option<u64> {
        self.debouncer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .next_due()
    }

    /// The timer: shows each burst as it comes due, then disarms (and looks once more for an op
    /// that landed in between).
    fn wait_and_show(self: Arc<Self>) {
        loop {
            while let Some(due) = self.next_due() {
                let wait = due.saturating_sub(now_ms());
                if wait > 0 {
                    std::thread::sleep(Duration::from_millis(wait));
                    continue;
                }
                self.flush_at(now_ms());
            }
            self.armed.store(false, Ordering::SeqCst);
            if self.next_due().is_none() || self.armed.swap(true, Ordering::SeqCst) {
                return;
            }
        }
    }
}

/// A Watch stream that holds a notice owner for as long as the client keeps it.
pub struct Owned<S> {
    inner: S,
    _owner: Option<OwnerGuard>,
}

impl<S> Owned<S> {
    /// `inner`, holding a notice owner while it lives when the client `shows_notices` (and a hub
    /// is installed): the guard drops with the stream when the client hangs up.
    pub fn of(shows_notices: bool, inner: S) -> Self {
        let owner = shows_notices.then(hub).flatten().map(Hub::owner);
        Owned {
            inner,
            _owner: owner,
        }
    }
}

impl<S: tokio_stream::Stream + Unpin> tokio_stream::Stream for Owned<S> {
    type Item = S::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<S::Item>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_next(cx)
    }
}

#[cfg(test)]
#[path = "notices_tests.rs"]
mod tests;
