## Goal

Replace the Stickies.app todo.txt workflow: desktop app stays running with a
persistent top-nav (menu bar / tray) icon, doesn't quit when the window
closes, and can optionally stay pinned above other windows.

## Design

- Tray icon: builds the item `desktop-quick-add/todo.txt` already named but
  never implemented (grep for `TrayIcon`/`SystemTray` in
  `apps/desktop/src-tauri/src/` turns up nothing — only the global hotkey
  shipped). One tray, menu: Open, Quick Add, Quit.
- Hide-not-quit: `apps/desktop/src-tauri/src/lib.rs` has no
  `on_window_event`/`CloseRequested` handler today, so closing the window
  quits the app (Tauri default). Add one that hides instead; only the tray's
  Quit item calls `app.exit()`.
- Stickiness: main window isn't `always_on_top` today — only the quick-add
  popover is (`desktop-quick-add/notes.md`). Add a titlebar/top-nav pin
  toggle calling `set_always_on_top`, persisted (reuse whatever local-setting
  mechanism the app already has, or add a small one if none exists).

## Known gap / not in scope here

- Launch-at-login isn't part of this task — flagged as a candidate follow-up
  if "always on" should also mean "runs after a reboot without opening it by
  hand," but wasn't asked for explicitly.

## Open question (step 3, @human)

- macOS dock icon: hide it once the app can live window-less (pure menu-bar
  app), or keep both dock and tray icons? Affects `tauri.conf.json`
  (`app.macOSPrivateApi` / activation policy) and is a product call, not an
  engineering one.
