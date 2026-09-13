# Menu-bar quick-add with a global hotkey (plan M7, plan §7 §3.2)

Plan M7: "Menu-bar quick-add (global hotkey) that opens only the popover editor." Design §7
clients table: desktop = "menu-bar quick-add; global hotkey". Plan §3.2 defines the popover: one
single-line CM6 editor with the same highlighting, token chips, strict-mode validation, footer with
`Line N · device, time`, Cancel/Save, Enter saves, Esc cancels, identical save is a no-op.

## Goal

One global hotkey opens the *same* popover as the main view, in a small always-on-top window, with
no document behind it. Quick-add is a second window around one shared component, not a second
editor.

## Design

### Extract the popover so the two surfaces cannot drift

```svelte
<!-- apps/desktop/src/Popover.svelte — shared by main view and quick-add -->
<script lang="ts">
  // props in, one event out; the component knows nothing about which window hosts it
  export let initialLine: string;          // pre-filled raw line (empty for quick-add)
  export let anchor: { file: string; line: number } | null;
  export let onSave: (raw: string) => Promise<SaveResult>;
  // SaveResult: { ok: true } | { ok: false; quirk?: Quirk; error: string } (lenient save, §3.2)
</script>
```

The popover is already being built by the edit-popover task (parent line 41); this task extracts it
into `Popover.svelte` and re-wires the main view to the same component. The quick-add window mounts
`Popover.svelte` with `initialLine = ""` and `anchor = null` (no `Line N` footer — there is no line
yet; the daemon stamps the creation date and `id:`).

### Tray + hotkey wiring (`apps/desktop/src-tauri/`)

```rust
// main.rs — Tauri 2 system tray + global shortcut.
// Ref: https://v2.tauri.app/learn/system-tray/ · https://v2.tauri.app/plugin/global-shortcut/
// The handler only *opens the window* and focuses the input; it never touches the file.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn Error>> {
    use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
    app.global_shortcut().on_shortcut(
        Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space),
        |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed { focus_quick_add(app); }
        },
    )?;
    Ok(())
}
```

- Hotkey choice: `Cmd/Ctrl+Shift+Space` by default, user-configurable later — keep the binding in
  one named constant so it is trivial to change.
- Submit path: `onSave` calls the same Tauri command the main-view popover uses (e.g.
  `apply_line(file, raw)`), which is a tonic `Apply` into the daemon. The daemon stamps the
  creation date and `id:` — the UI never writes the file itself (design §7: thin clients).
- Esc / Cancel closes the window with **no** op-log entry; an empty submit is also a no-op.

## Placement / dependencies

- New/extracted: `apps/desktop/src/Popover.svelte` (moved out of the main view), a small
  `apps/desktop/src/QuickAdd.svelte` window that mounts it, and tray/hotkey setup in
  `apps/desktop/src-tauri/main.rs`.
- Depends on: the edit-popover task (line 41) for the popover internals, the daemon's `Apply`
  RPC, and two Tauri plugins — `tauri-plugin-global-shortcut` and the built-in tray API (npm +
  Cargo deps; human sign-off + `deny.toml` pass before landing).

## Edge cases & invariants

- Hotkey must open quick-add even when another app has focus — that is the whole point of a
  *global* shortcut; test it with the window unfocused.
- The shortcut must not fire while the popover already has an unsaved edit in the main window
  (guard: if the main popover is open and dirty, the hotkey focuses the main window instead).
- Re-register/re-arm the shortcut when the quick-add window hides, or the hotkey silently dies
  after first use — a classic plugin gotcha.
- Identical-or-empty save writes nothing; assert no new op-log entry appears.
- The quick-add window never parses the line itself; validation runs through the same WASM
  `txtodo-ffi` strict-mode path as the main popover (plan §3.2), so a lenient save with a quirk
  behaves identically in both windows.

## Acceptance

- Hotkey (or a direct `focus_quick_add` command in the test) opens the popover while another app
  is focused, with the input focused and empty.
- Enter saves exactly one line through the daemon; an on-disk byte diff shows only that line
  changed and the new line carries a stamped creation date + `id:`.
- Esc dismisses with nothing written; an empty submit also writes nothing.
- The main view and quick-add render the same `Popover.svelte` (assert via one component, not two).

## References

- Plan §3.2 popover · plan M7 quick-add · design §7 clients table.
- https://v2.tauri.app/learn/system-tray/ · https://v2.tauri.app/plugin/global-shortcut/
- https://v2.tauri.app/develop/calling-rust/ · https://codemirror.net/

## As built (2026-09-13, agent)

Built from scratch this session, on top of the already-built `EditPopover`.

### Shared popover: reused `EditPopover.svelte`, did not add a `Popover.svelte`

This file's design names the extracted component `Popover.svelte`. `EditPopover.svelte` (built by
an earlier session for `desktop-edit-popover`) already matched that exact contract once its
`taskRef`/`onSave` were widened this session (see `desktop-edit-popover`'s "As built" for the
bugfix that made this safe: it no longer applies its own write, so a host can point it at either an
`Edit` or an `Add`). Renaming it, or forking a second `Popover.svelte` that re-exports it, would
have added a file with no behavioral difference — kept the one name the main view, detail view, and
now quick-add all already import, so there is exactly one popover implementation to drift.

### Frontend

- `apps/desktop/src/lib/components/QuickAdd.svelte` — mounts `EditPopover` with `taskRef={null}`,
  `initialLine=""`, `anchor={null}`, targeting the root `todo.txt` via a plain `Add` mutation.
  `{#key showCount}` remounts it (fresh empty input, refocused) on every `quick-add-shown` event —
  necessary because the window is hidden between uses, not destroyed, so its Svelte component would
  otherwise carry over whatever was left in it from the previous open.
- `apps/desktop/src/routes/+page.svelte` — chooses `QuickAdd` vs `MainView` by
  `getCurrentWindow().label === "quick-add"` at mount, rather than adding a second SvelteKit route.
  Deliberate: `adapter-static`'s SPA fallback makes a second route's prerendering behavior under
  Tauri's asset protocol something to re-verify every time the build config changes, whereas one
  static build serving two windows by label is exactly Tauri's own default single-window pattern,
  just checked at runtime.

### Rust (`apps/desktop/src-tauri/src/quick_add.rs`, new module)

- `create_window`: a hidden, `always_on_top`, undecorated, `skip_taskbar` window (label
  `"quick-add"`), created once at startup — hidden, not destroyed, between uses.
- `register_shortcut`: `Cmd/Ctrl+Shift+Space` via `tauri_plugin_global_shortcut`'s persistent
  `on_shortcut` handler (registered once, not re-armed per press — this API shape doesn't have the
  notes' warned "silently dies after first use" failure mode, since it never unregisters itself).
  On `Pressed`: if the main popover is dirty, focuses the main window instead of opening quick-add;
  otherwise shows+focuses quick-add and emits `quick-add-shown`.
- `set_main_popover_dirty` Tauri command + `AppState.main_popover_dirty: AtomicBool` (a plain
  atomic, not the crate's usual `tokio::sync::Mutex`, because the shortcut handler that reads it is
  a synchronous callback) — wired from `EditPopover`'s new `onDirtyChange` prop via `MainView`.
- New Cargo dependency `tauri-plugin-global-shortcut = "2"` (human sign-off + `cargo deny` pass
  needed, flagged in `Cargo.toml`'s own comment) and a new capability
  `apps/desktop/src-tauri/capabilities/quick-add.json` scoping `core:window:allow-{hide,show,
  set-focus}` to the `"quick-add"` window only.

### Edge cases from the notes, and how each is actually handled

- **Opens while unfocused**: the OS-level global shortcut is registered process-wide by the
  plugin; this is what "global" means, but I could not verify the *actual OS focus-stealing
  behavior* myself (see "what to open and look at" below — this is exactly the kind of check this
  repo's CLAUDE.md reserves for a human, not an automated headless run).
- **Guard against clobbering an unsaved main-window edit**: implemented via the dirty-flag command
  above; also unverifiable end-to-end without real OS windows.
- **Re-arm after hide**: not applicable to this plugin's API shape — see `register_shortcut`'s doc
  comment.
- **Identical-or-empty save writes nothing**: inherited for free from `EditPopover`'s existing
  `isNoOpEdit(initialLine, next)` check against `initialLine = ""` — an empty submit is already a
  no-op today, unit-tested in `editPopoverLogic.test.ts` (unchanged, this session added no new
  logic here, only reused it).
- **Same `Popover.svelte` in both windows**: literally true here — see above.

### Tests

- No new Playwright scenario: quick-add's own acceptance needs a real OS-level global hotkey and a
  second native window, neither of which a browser-only Playwright harness (see
  `desktop-playwright-tests`) can drive — this is a `tauri-driver`-shaped verification, explicitly
  out of scope per this repo's "don't launch/drive the app to look at it" rule.
- No new vitest file: the only new pure logic (`EditPopover`'s dirty check) is a one-line string
  comparison already exercised by `isNoOpEdit`'s existing tests.
- Verified instead via `cargo build -p desktop`, `cargo clippy -p desktop --all-targets -- -D
  warnings`, `cargo test -p desktop`, and `npm run check`/`vitest run` — all green.

## What to open and look at

This is the one piece in this milestone that genuinely needs a human's eyes and hands — it's OS
window/hotkey behavior, which nothing short of `tauri-driver` (out of scope, see above) or a human
running the real app can confirm:

- `npm run tauri dev` (or a built app). Focus a different application entirely, then press
  `Cmd+Shift+Space` (macOS) / `Ctrl+Shift+Space` (Linux/Windows): a small, undecorated,
  always-on-top window should appear with an empty, focused single-line editor.
- Type a task and press Enter: the window should disappear, and the main window's `todo.txt`
  should gain exactly one new line (with a stamped date and, per the workspace's identity mode, an
  `id:` tag or not — see `desktop-detail-view`'s notes on `identity_mode`).
- Press the hotkey again with nothing typed, then Esc: nothing should be written.
- Open the main window's edit popover on some line, type something into it (don't save), then press
  the global hotkey: the main window should come forward instead of quick-add opening on top of it.
- Press the hotkey a second and third time (after using it once): it should keep working — this is
  the "does it silently die after first use" check the notes call out.
