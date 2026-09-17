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

## Open question (step 3, @human) — resolved 2026-09-18

Decided (per the overnight task brief that authorized this build, matching the
"don't re-litigate" instruction it carried): keep the macOS dock icon visible
always. Do not switch the app to an accessory/`LSUIElement`/menu-bar-only app.
No `tauri.conf.json` activation-policy change made. Rationale given: keeps
behavior simple and avoids extra platform-specific activation-policy code —
both tray and dock icon coexist, matching how many "menu bar extra" apps also
keep a normal dock presence. Revisit if a human wants a true menu-bar-only
feel later; it's a `tauri.conf.json`/`ActivationPolicy` change, not touched
here.

## As built (2026-09-18, agent)

- **Tray icon + menu** (`apps/desktop/src-tauri/src/tray.rs`, new file):
  `tauri::tray::TrayIconBuilder` with three items — Open, Quick Add,
  separator, Quit — via `tauri::menu::{Menu, MenuItem, PredefinedMenuItem}`.
  Built once in `lib.rs`'s `.setup()` (`tray::create_tray(app.handle())?`),
  after the main window and quick-add window exist. The returned `TrayIcon`
  is `app.manage()`d so it isn't dropped (its `Drop` impl would unregister the
  icon) — same "must outlive `run()`" reasoning as `AppState`. Falls back
  gracefully to the platform's own default tray glyph if
  `app.default_window_icon()` is `None` (never fails setup over a cosmetic
  gap). "Open" shows+focuses the main window; "Quick Add" reuses
  `quick_add::focus_quick_add` (widened from `fn` to `pub(crate) fn` — no
  behavior change, same function); "Quit" is the only path that calls
  `app.exit(0)`. `Cargo.toml`'s `tauri` dependency gained the `tray-icon`
  feature flag (required by `tauri::tray`/`tauri::menu`).
- **Hide-not-quit** (`lib.rs::install_hide_not_quit`, called from `.setup()`
  right after the tray is built): `main` window's `on_window_event` intercepts
  `WindowEvent::CloseRequested`, calls `api.prevent_close()` then
  `window.hide()`. The window is never destroyed, only hidden — reopened by
  the tray's "Open" item or the dock icon, same as `quick_add`'s own
  hide-not-destroy pattern for the popover.
- **Pin-on-top** (`apps/desktop/src-tauri/src/commands_window.rs`, new file,
  one command `set_pinned(window, pinned) -> Result<(), String>` calling
  `window.set_always_on_top(pinned)`; `apps/desktop/src/lib/stores/pin.ts`,
  new file, frontend-owned persistence): the boolean is persisted to
  `localStorage` under `txtodo-pinned`, the exact same mechanism
  `$lib/stores/theme.ts` already uses for the theme preference — this app has
  no Rust-side settings file to instead add a second persistence path to.
  Deliberately uses **relative** imports (`../tauriShim`, no `$app/
  environment`) rather than `theme.ts`'s `$lib`/`$app` aliases, specifically
  so `pin.test.ts` can unit-test it: this repo's `vitest.config.ts` has no
  SvelteKit plugin, so `$lib`/`$app` aliases only resolve through the real
  SvelteKit build, not under `vitest` (confirmed by trying `$app/environment`
  first and watching module resolution fail under `vitest run`). A new
  `PinToggle.svelte` component (styled to match `ThemeToggle.svelte`) sits
  next to it in `MainView.svelte`'s top-nav; `applyStoredPin()` runs once from
  `MainView`'s `onMount` (a freshly created OS window always starts unpinned,
  so the stored preference has to be re-applied, not just read).
  `mock/tauriMock.ts` gained a `"set_pinned"` case (no-op — there is no real
  window to pin in the browser-preview mock).

## Test coverage — what's real vs `@human`-flagged

- **Automated, passing:** `apps/desktop/src/lib/stores/pin.test.ts` (5 tests)
  covers the actually-automatable half of item 6 — "pin toggle round-trips
  through a restart" — by simulating a restart as a fresh module import
  against the same in-memory `localStorage` stand-in, asserting
  `readStoredPreference`/`pinned`/`applyStoredPin` all agree after the
  simulated relaunch, and that a failed IPC call never throws. Runs under
  plain Node (no jsdom): Node has no default global `localStorage` without an
  experimental flag this repo doesn't set, so the test installs a minimal
  in-memory `Storage` stand-in itself rather than adding a jsdom/happy-dom
  devDependency for one file.
  `cargo test -p desktop` (17 tests, unchanged) and `cargo clippy -p desktop
  --all-targets -- -D warnings`, `npm run check` (0 errors), `npm run build`,
  and the full `npx vitest run` (113 tests) all stayed green through this
  work.
- **Not automated, `@human`-flagged (items 5 and 6's other half):** neither
  "closing the main window hides it and leaves the tray running" nor "the
  window actually stays above other windows when pinned" can be asserted by
  anything in this repo's test harnesses. `apps/desktop`'s Playwright suite
  (`e2e/*.spec.ts`) drives a plain Chromium tab against `vite dev --mode e2e`
  — it never touches a real native window, tray icon, or `set_always_on_top`
  effect at all (those are OS/Tauri-runtime concepts a browser tab has no
  access to), so writing a spec there would only assert against the mock,
  not the real behavior it's meant to verify. Per this repo's own
  expensive-visual-check convention (and the standing instruction not to
  spin up browsers/GUIs to self-verify without asking), no such spec was
  added. **A human needs to, once, launch the packaged app and confirm:**
  (1) the tray icon renders and its three menu items work, (2) clicking the
  main window's close button hides it rather than quitting (check Activity
  Monitor/`ps` — the process should still be running, and the tray icon
  should still respond), (3) tray "Quit" is what actually ends the process,
  (4) toggling Pin visibly keeps the window above others, and it survives a
  full quit/relaunch. None of this was verified by the agent building it —
  this sandbox has no display/window manager to drive Tauri's real webview
  at all.
