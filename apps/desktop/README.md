# txtodo desktop

- A [Tauri 2](https://v2.tauri.app/) + SvelteKit desktop shell for [txtodo](../../README.md).
  - Rust side (`src-tauri/`) owns the connection to `txtodod`, the per-workspace sync daemon.
  - Svelte side (`src/`) never touches a file, path, or socket directly — everything crosses
    the Tauri command bridge (design §7).

## Capabilities

- **Auto-starts the daemon.** On launch, dials `<workspace>/.txtodo/txtodod.sock`; spawns
  `txtodod --dir <workspace>` if nothing answers. A banner shows `spawning` / `connecting` /
  `dead` and offers Retry — the daemon being absent is a state, never a crash.
- **One shell, three pages.** A header with the workspace menu, a search box, and Tasks /
  Universal / Settings; a prompt bar at the foot of every page; a status line with save state and
  the caret. Ink-on-paper light and dark themes (Tailwind 4 + DaisyUI 5, components prefixed
  `d-`). Design record: `docs/design/desktop-ui-revamp/`.
- **Live, editable buffer.** The root list is a CodeMirror 6 buffer you edit in place, todo.txt
  syntax coloured, saved on the fly and on blur. Updates arrive over a `Watch` stream. Double-click
  or Cmd/Ctrl+Enter opens a line's detail (sub-list or notes) in a panel below.
- **Search.** The header search matches with the shared core (words, `-word`, `is:open`,
  `is:done`): it marks and steps through hits on Tasks, filters rows on Universal and cards on
  Settings.
- **Prompt bar and Quick Add.** Enter adds to the root list; chips insert `(A)`, `x`, `due:`
  and friends by the core's rules (the TUI's too). The mark beside it fills as tasks get done.
  The global hotkey (Cmd/Ctrl+Shift+Space) focuses the bar when the app is in front, else opens
  the Quick Add window, which is the same bar.
- **Universal.** Tasks from every workspace: stats, group by priority / due / project / context /
  workspace, chips, show done, due badges, `ref:` progress; complete or reopen in the row's own
  workspace, with Undo; keyboard-first (j/k, Enter, x).
- **Conflict review.** A banner reports open `needs_review` flags (two devices edited the same
  word); the review sheet shows a diff and lets you resolve `mine` / `theirs` / `merged`.
- **Settings.** General, Appearance (theme and editor prefs with a live preview), Workspaces,
  Shortcuts (generated from `src/lib/keyTable.ts`, the same table `/help` shows), Devices, Tokens
  and Activity, with a sticky section nav and `/settings#<card>` deep links.
- **Device pairing.** Show a QR code (`pair_offer`) or scan a peer's with the camera
  (`pair_accept`), then compare a 6-word SAS out loud and confirm on **both** devices —
  confirmation is always an explicit second tap, never automatic. No group key or private key
  ever appears in the QR payload. Settings lists paired devices; removing one rotates the group
  key (`DeviceRemove`).
- **Capability tokens.** Create named, scoped, revocable bearer tokens (`read`, `write:add`,
  `write:complete`, `write:edit`, `write:delete`, `raw`, plus `project:`/`context:`/`file:`
  restrictors) — e.g. for handing an agent write access to one file only. The secret is shown
  once, at creation, and never again.
- **Activity feed.** The newest ops across every workspace, newest first, with who did what and
  when (Settings > Activity). One bounded fetch (refresh button + refetch on window focus), not a
  live tail.
- **Always-on background app.** A persistent tray/menu-bar icon (Open, Quick Add, Quit) keeps the
  app reachable after the window closes — closing the main window hides it rather than quitting;
  the tray's Quit item is the only thing that actually exits. An optional pin-on-top toggle in the
  header keeps the window above others, persisted across restarts. The macOS dock icon stays
  visible always (a deliberate choice, not a true menu-bar-only app — see
  `tasks/desktop-always-on/notes.md`).

## Requirements

- Rust (workspace `rust-version` — see root `Cargo.toml`) and Node.js.
- `txtodod` reachable one of three ways, checked in this order (`config.rs::sidecar_daemon_bin`,
  `daemon/spawn.rs`): a Tauri sidecar bundled alongside the packaged app (`tauri.conf.json`'s
  `bundle.externalBin`, task `desktop-daemon-sidecar-bundle` — what a real install gets, no
  manual step); a binary sitting beside this process' own executable (what a plain `cargo build
  --workspace` dev checkout gets for free, since both land in the same `target/debug/`); or
  `$PATH` (the old, still-supported fallback). `just stage-desktop-sidecar` builds and stages this
  host's own `txtodod` as the sidecar for a local `npm run tauri build` — see that recipe's own
  comment for what CI does per release target instead.
- macOS/Linux only for now — the sync transport is unix-socket only; Windows named pipes land
  in plan M10.
- Webview floor: macOS 13.3+ (WKWebView as Safari 16.4; `bundle.macOS.minimumSystemVersion`), or
  Linux with webkit2gtk-4.1 2.40+. The UI's CSS (Tailwind 4, `color-mix()`, `:has()`, `@layer`,
  `oklch()`) needs it; Vite's `build.cssTarget` is set to match.

## Development

```bash
npm install
npm run tauri dev
```

- `npm run dev` — Vite dev server only (frontend, no Tauri window).
- `npm run tauri dev` — the actual app, with hot reload.
- `npm run tauri build` — production bundle (`bundle.targets: "all"` in `tauri.conf.json`).
- `npm run check` — `svelte-kit sync` + `svelte-check`.
- `npm test` — Vitest unit tests (`src/**/*.test.ts`).
- `cargo test -p desktop` — Rust-side integration tests (`src-tauri/tests/`).
- `npm run build:wasm-core` — regenerates `src/lib/wasm-core/` from `crates/txtodo-ffi`
  (needs `rustup target add wasm32-unknown-unknown` + `wasm-bindgen-cli`). Generated output,
  committed but never hand-edited.

## Parity with the TUI

`specs/client-parity.toml` is the one list of user-facing actions and where this app and the TUI
each stand on them (ADR 0031, `tasks/tui-revamp/parity-manifest`).

- Change the manifest in the same commit as any key, shortcut or user-facing action you add,
  change or drop here.
- If the TUI lags, add an `@parity` line naming the action id to its backlog
  (`tasks/tui-revamp/todo.txt`); if this app lags, to `tasks/desktop-ui-revamp/todo.txt`.
- Never let the two drift silently: a different key or a missing feature is a `differs` or `na`
  row with its one-line `deviation`.
- `src/lib/keys.parity.test.ts` checks the manifest's rules now, and `keys.ts` against it once the
  revamp creates `keys.ts`. The Help page and the Shortcuts card render from the manifest.

## Configuration

- `TXTODO_WORKSPACE` — workspace directory to talk to (dev/test override). Defaults to the
  current directory at launch.

## Layout

```
src-tauri/src/
  lib.rs               entry point: registers commands, kicks off first connect
  daemon.rs, daemon/spawn.rs   DaemonClient + ensure_daemon (spawn/probe/lock)
  commands*.rs         one #[tauri::command] fn per daemon RPC, grouped by feature
  dto*.rs              serde DTOs crossing the IPC bridge (prost types aren't Serialize)
src/
  lib/daemon.ts         typed wrappers over every Tauri command + event
  lib/components/       AppShell, MainView, FileView (CodeMirror), PromptBar + LineEditor,
                        Universal*, settings/*, conflict UI
  lib/keys.ts, keyTable.ts   shell bindings and the shortcuts reference
  lib/universal/, lib/settings/   pure models for those pages (Vitest)
  lib/lang/              todo.txt Lezer grammar for syntax highlighting
  lib/wasm-core/         generated bindings to crates/txtodo-ffi (diffing, etc.)
  devices/               pairing, paired devices and tokens (Settings cards)
  routes/                /, /universal, /settings, /help (/devices redirects to Settings)
```
