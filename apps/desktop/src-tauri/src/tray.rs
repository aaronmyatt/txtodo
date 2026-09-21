//! The persistent tray (menu-bar) icon (task `desktop-always-on`): "Open", "Quick Add", "Quit".
//! Together with `lib.rs`'s hide-not-quit `CloseRequested` handler, this is what lets the app
//! stay running (and reachable) after the main window is closed — closing the window only hides
//! it now; the tray's own "Quit" item is the one remaining way to actually exit the process.
//! Ref: <https://v2.tauri.app/learn/system-tray/>

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

const OPEN_ID: &str = "open";
const QUICK_ADD_ID: &str = "quick_add";
const QUIT_ID: &str = "quit";

/// Builds and registers the tray icon. `lib.rs`'s `.setup()` calls this once at startup, after
/// the main window exists (`show_main_window` below assumes `get_webview_window("main")` can
/// find it). The returned `TrayIcon` is handed to `app.manage(..)` by the caller — dropping it
/// would unregister the icon (`TrayIcon`'s own `Drop` impl), so it must be kept alive for the
/// app's lifetime, the same reason `AppState` is `app.manage`d rather than left on the stack.
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, OPEN_ID, "Open", true, None::<&str>)?;
    let quick_add_item = MenuItem::with_id(app, QUICK_ADD_ID, "Quick Add", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &quick_add_item, &separator, &quit_item])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(on_menu_event);
    // Best effort: a missing default window icon (e.g. a stripped-down test build with no bundle
    // icons configured) still gets a tray entry with the platform's own fallback glyph rather
    // than failing the whole setup.
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    app.manage(tray);
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        OPEN_ID => show_main_window(app),
        QUICK_ADD_ID => crate::quick_add::focus_quick_add(app),
        QUIT_ID => app.exit(0),
        _ => {}
    }
}

/// Shows and focuses the main window — the tray's "Open" item, and the counterpart to `lib.rs`'s
/// hide-on-close handler (which only hides, never destroys, the window this brings back).
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
