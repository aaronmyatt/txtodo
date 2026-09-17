# txtodo desktop

- A [Tauri 2](https://v2.tauri.app/) + SvelteKit desktop shell for [txtodo](../../README.md).
  - Rust side (`src-tauri/`) owns the connection to `txtodod`, the per-workspace sync daemon.
  - Svelte side (`src/`) never touches a file, path, or socket directly — everything crosses
    the Tauri command bridge (design §7).

## Capabilities

- **Auto-starts the daemon.** On launch, dials `<workspace>/.txtodo/txtodod.sock`; spawns
  `txtodod --dir <workspace>` if nothing answers. A banner shows `spawning` / `connecting` /
  `dead` and offers Retry — the daemon being absent is a state, never a crash.
- **Live file view.** Renders `todo.txt` read-only in CodeMirror 6, todo.txt syntax highlighted
  (priority, dates, `+project`, `@context`, `key:val`). Updates in real time over a `Watch`
  stream — no manual refresh.
- **Click-to-edit popover.** Click a line to edit its raw text; saves as an `Apply` mutation
  addressed by line number + ULID task id, so a stale edit against a moved/deleted line is
  rejected rather than silently corrupting the wrong line.
- **Conflict review.** A banner reports open `needs_review` flags (two devices edited the same
  word); the review sheet shows a diff and lets you resolve `mine` / `theirs` / `merged`.
- **Device pairing.** Show a QR code (`pair_offer`) or scan a peer's with the camera
  (`pair_accept`), then compare a 6-word SAS out loud and confirm on **both** devices —
  confirmation is always an explicit second tap, never automatic. No group key or private key
  ever appears in the QR payload.
- **Capability tokens.** Create named, scoped, revocable bearer tokens (`read`, `write:add`,
  `write:complete`, `write:edit`, `write:delete`, `raw`, plus `project:`/`context:`/`file:`
  restrictors) — e.g. for handing an agent write access to one file only. The secret is shown
  once, at creation, and never again.
- **Activity feed.** The newest ~200 ops across the workspace, newest first, with who did what
  and when. One bounded fetch (refresh button + refetch on window focus), not a live tail.
- **Always-on background app.** A persistent tray/menu-bar icon (Open, Quick Add, Quit) keeps the
  app reachable after the window closes — closing the main window hides it rather than quitting;
  the tray's Quit item is the only thing that actually exits. An optional pin-on-top toggle in the
  top nav keeps the window above others, persisted across restarts. The macOS dock icon stays
  visible always (a deliberate choice, not a true menu-bar-only app — see
  `tasks/desktop-always-on/notes.md`).

## Requirements

- Rust (workspace `rust-version` — see root `Cargo.toml`) and Node.js.
- `txtodod` on `PATH` (built by `cargo build --workspace`), unless overriding `daemon_bin` in
  code for a test.
- macOS/Linux only for now — the sync transport is unix-socket only; Windows named pipes land
  in plan M10.

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
  lib/components/       MainView, FileView (CodeMirror), EditPopover, conflict UI
  lib/lang/              todo.txt Lezer grammar for syntax highlighting
  lib/wasm-core/         generated bindings to crates/txtodo-ffi (diffing, etc.)
  devices/               pairing, tokens, and activity-feed panes (routes/devices)
```
