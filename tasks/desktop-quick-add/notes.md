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
