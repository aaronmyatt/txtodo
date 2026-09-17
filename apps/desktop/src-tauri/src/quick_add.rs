//! Menu-bar quick-add: a hidden, always-on-top window mounting the shared `EditPopover`, shown
//! and focused by a global hotkey (tasks/desktop-quick-add/notes.md, plan M7, plan §7 §3.2). This
//! module only opens/focuses windows and reads the "is the main popover dirty" guard — it never
//! touches the daemon or the file itself (design §7: that's the frontend's `Apply` call via
//! `$lib/daemon.ts`, the exact same path the main view's popover uses).
//! Refs: <https://v2.tauri.app/learn/system-tray/> · <https://v2.tauri.app/plugin/global-shortcut/>

use crate::state::AppState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{
    Code, Error as ShortcutError, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};

/// Window label for the quick-add popover; `src/routes/+page.svelte` checks
/// `getCurrentWindow().label === "quick-add"` against this same string to decide whether to mount
/// `QuickAdd.svelte` instead of `MainView.svelte`, and `capabilities/quick-add.json` scopes its
/// permissions to it.
pub const QUICK_ADD_LABEL: &str = "quick-add";

/// Event the frontend listens for to reset/refocus the popover on every show (`QuickAdd.svelte`)
/// — this window is hidden between uses, never destroyed, so its Svelte app only mounts once;
/// each hotkey press still needs a fresh, empty, focused input.
const QUICK_ADD_SHOWN_EVENT: &str = "quick-add-shown";

/// `Cmd/Ctrl+Shift+Space` (notes.md's default, "user-configurable later") — kept as one named
/// function so changing the binding later is a one-line edit, not a hunt through `setup`.
fn quick_add_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space)
}

/// Creates the quick-add window hidden; `lib.rs`'s `setup` calls this once at startup. Hidden
/// (not destroyed) between uses so re-showing it never re-pays webview/JS startup cost — the
/// `quick-add-shown` event on each show is what keeps that safe rather than stale (see
/// `QuickAdd.svelte`'s module doc for the frontend half).
pub fn create_window(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, QUICK_ADD_LABEL, WebviewUrl::App("index.html".into()))
        .title("Quick add")
        .inner_size(420.0, 120.0)
        .resizable(false)
        .always_on_top(true)
        .decorations(false)
        .skip_taskbar(true)
        .visible(false)
        .build()?;
    Ok(())
}

/// Registers the global hotkey. Uses the plugin's persistent `on_shortcut` handler — registered
/// once here, not re-armed per press — so notes.md's "re-register/re-arm the shortcut when the
/// quick-add window hides, or the hotkey silently dies after first use" gotcha doesn't apply to
/// this API shape: that warning is about a manual register/unregister-per-use pattern, and
/// `on_shortcut` doesn't unregister itself after firing, so there is nothing to re-arm.
pub fn register_shortcut(app: &AppHandle) -> Result<(), ShortcutError> {
    app.global_shortcut()
        .on_shortcut(quick_add_shortcut(), |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return; // fire on key-down only; ignore the matching key-up event
            }
            // Guard (notes.md): "the shortcut must not fire while the popover already has an
            // unsaved edit in the main window" — `set_main_popover_dirty` keeps this in sync from
            // `EditPopover`'s `onDirtyChange`.
            let dirty = app
                .state::<AppState>()
                .main_popover_dirty
                .load(Ordering::Relaxed);
            if dirty {
                focus_main_window(app);
            } else {
                focus_quick_add(app);
            }
        })
}

/// Shows and focuses the quick-add window, then tells its frontend to reset/refocus the input.
/// `pub(crate)` (not private) so `tray.rs`'s "Quick Add" menu item (task `desktop-always-on`) can
/// reuse the exact same show/focus/reset sequence the global hotkey uses, rather than a second
/// copy.
pub(crate) fn focus_quick_add(app: &AppHandle) {
    let Some(window) = app.get_webview_window(QUICK_ADD_LABEL) else {
        return; // create_window() runs at startup; only absent if that itself failed
    };
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit_to(QUICK_ADD_LABEL, QUICK_ADD_SHOWN_EVENT, ());
}

/// The guarded path: the main popover is mid-edit, so the hotkey brings that window forward
/// instead of opening a second, unrelated editor on top of it.
fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
