//! Window-stickiness command (task `desktop-always-on`): the frontend's pin toggle persists its
//! own boolean to `localStorage` (same pattern `$lib/stores/theme.ts` already uses for the theme
//! preference — see that module's doc comment) and calls [`set_pinned`] to apply it live. This
//! crate has no Rust-side settings file of its own to add a second persistence mechanism to; the
//! frontend already owns "restore this preference on the next launch" for exactly this kind of
//! per-viewer UI setting.

use tauri::Window;

/// Pins or unpins the main window above other windows. `window` resolves to whichever window
/// invoked the command; the frontend only ever calls this from the main window, so there is no
/// separate "target the main window by label" lookup needed here (unlike `tray.rs`'s tray-menu
/// handlers, which have no invoking window to resolve from).
#[tracing::instrument(name = "ipc.set_pinned", skip_all)]
#[tauri::command]
pub fn set_pinned(window: Window, pinned: bool) -> Result<(), String> {
    window.set_always_on_top(pinned).map_err(|e| e.to_string())
}
