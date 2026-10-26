//! OS notifications for another client's changes (task change-notifications): every `Watch`
//! change's ops go through `txtodo_core::notice::Debouncer` (shared with the TUI), which drops this
//! app's own (source `desktop`) and folds a burst into one notice. One timer task waits for the
//! burst to go quiet, then shows the notice unless the main window has focus (the human is
//! looking at it already) or Settings turned notices off.
//! Ref: https://v2.tauri.app/plugin/notification/

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use txtodo_core::notice::{Debouncer, OpFacts};
use txtodo_proto::v1 as pb;

/// The notices' state, held in `AppState`.
pub struct Notices {
    debouncer: Mutex<Debouncer>,
    /// Settings > General's toggle; on by default.
    pub enabled: AtomicBool,
    /// A timer task is waiting for the current burst.
    armed: AtomicBool,
}

impl Default for Notices {
    fn default() -> Self {
        Notices {
            debouncer: Mutex::new(Debouncer::new("desktop")),
            enabled: AtomicBool::new(true),
            armed: AtomicBool::new(false),
        }
    }
}

/// Wall-clock ms; the debouncer only compares them with each other.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Feeds one change's ops in and, when one counted, starts the timer if none is waiting.
pub fn on_change(app: &AppHandle, change: &pb::Change) {
    let notices = &app.state::<crate::state::AppState>().notices;
    if !notices.enabled.load(Ordering::Relaxed) {
        return;
    }
    let now = now_ms();
    let mut counted = false;
    {
        let mut d = lock(notices);
        // Every op goes in (no short-circuit): one that does not count still must not be skipped.
        for op in &change.ops {
            counted |= d.push(
                now,
                OpFacts {
                    path: &change.path,
                    source: &op.source,
                    kind: &op.kind,
                    task_id: &op.task_id,
                    summary: &op.summary,
                },
            );
        }
    }
    if counted && !notices.armed.swap(true, Ordering::SeqCst) {
        tauri::async_runtime::spawn(wait_and_show(app.clone()));
    }
}

/// Shows each burst as it comes due, then disarms. An op that lands between the last look and
/// the disarm found the timer still armed, so it looks once more before it stops.
async fn wait_and_show(app: AppHandle) {
    loop {
        show_bursts(&app).await;
        let notices = &app.state::<crate::state::AppState>().notices;
        notices.armed.store(false, Ordering::SeqCst);
        let pending = lock(notices).next_due().is_some();
        if !pending || notices.armed.swap(true, Ordering::SeqCst) {
            return;
        }
    }
}

/// Sleeps until the pending burst is due and shows it, until none is pending.
async fn show_bursts(app: &AppHandle) {
    loop {
        let notices = &app.state::<crate::state::AppState>().notices;
        let Some(due) = lock(notices).next_due() else {
            return;
        };
        let wait = due.saturating_sub(now_ms());
        if wait > 0 {
            tokio::time::sleep(Duration::from_millis(wait)).await;
            continue;
        }
        let notice = lock(notices).due(now_ms());
        if let Some(n) = notice {
            show(app, &n.title, &n.body);
        }
    }
}

fn lock(notices: &Notices) -> std::sync::MutexGuard<'_, Debouncer> {
    notices
        .debouncer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One OS notification, unless the main window has focus or notices are off. A failure is logged,
/// never shown: a missed notice must not disturb the app.
fn show(app: &AppHandle, title: &str, body: &str) {
    let notices = &app.state::<crate::state::AppState>().notices;
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if focused || !notices.enabled.load(Ordering::Relaxed) {
        return;
    }
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(error = %e, "notice_failed");
    }
}

/// Settings > General's toggle (`$lib/stores/changeNotices.ts` mirrors it here at start and on
/// every change).
#[tauri::command]
pub fn set_change_notices(state: tauri::State<'_, crate::state::AppState>, enabled: bool) {
    state.notices.enabled.store(enabled, Ordering::Relaxed);
}
