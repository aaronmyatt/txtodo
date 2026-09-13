# Playwright tests for popover, detail, notes creation, nested breadcrumb, conflict sheet (plan M7, plan §3.2 §7)

Plan M7 acceptance: "Playwright/WebDriver tests: click opens popover with the raw line including
`id:`; Enter saves and the file on disk changes only that line; double-click opens detail; typing
into empty notes creates the directory; sub-list line double-click nests the breadcrumb; conflict
sheet appears when the test injects concurrent ops via a second daemon."

## Goal

Five browser scenarios (plus the conflict case) driven against a **real** `txtodod`, not a stubbed
reducer. The point of the suite is that the Svelte UI, the Tauri-command proxy, the daemon, and the
reconciler all compose correctly — so the harness runs the real daemon on a tempdir workspace and
asserts both DOM and on-disk bytes.

## Design

### Harness: real daemon, seeded tempdir, one bridge

```ts
// apps/desktop/playwright.config.ts — two projects later (visual task), one here.
import { defineConfig } from "@playwright/test"; // https://playwright.dev/
export default defineConfig({
  testDir: "./e2e",
  use: { baseURL: "http://127.0.0.1:4173" }, // vite dev/preview serving the Svelte app
  webServer: { command: "npm run dev", port: 4173 },
});

// apps/desktop/e2e/fixtures.ts
// Spawn txtodod on tempfile::tempdir() seeded with fixture todo.txt, then point the app at it.
// The Tauri commands are a thin gRPC proxy (design §7); in the browser harness we drive the same
// gRPC directly, so the code under test is the real reconciler, real file bytes, real op log.
export async function spawnDaemon(fixture: "todo.txt" | "nested" | "conflict"): Promise<DaemonHandle>
```

Because Tauri commands don't exist in a plain browser, the fixture either (a) runs the app under
`tauri-driver` (WebDriver, https://v2.tauri.app/develop/tests/webdriver/) or (b) exposes the same
command surface through a thin test bridge that calls the daemon's gRPC the same way `commands.rs`
does. Pick **one**; the acceptance requires real reconciliation either way, not mocks.

### The five scenarios (each a `.spec.ts`)

| Scenario | Assertion (plan M7 wording) |
|---|---|
| `popover.spec.ts` | click a line → popover pre-filled with the **raw** line including the hidden `id:` tag |
| `save.spec.ts` | Enter saves; on-disk file changes **only that line** — byte-diff before/after |
| `detail.spec.ts` | double-click → detail view with pinned parent, notes editor, sub-list |
| `notes-create.spec.ts` | first keystroke into empty notes creates the `ref:` dir + `notes.md` (§3.2.4 lazy) |
| `breadcrumb.spec.ts` | double-click a sub-list line → breadcrumb `todo.txt › N › q4-roadmap/todo.txt › M` |
| `conflict.spec.ts` | a second daemon injects concurrent ops → `needs_review` → conflict sheet (M4 variants) |

- Byte-diff assertions read the file through the daemon (or directly from the tempdir) and compare
  against the pre-action snapshot — never assert only the DOM.
- Notes creation asserts the negative too: opening the detail view alone writes nothing; only the
  first keystroke creates the directory (lazy creation is creation-on-write, plan §3.2.4).
- Conflict scenario: spawn a second daemon on a different port/dir, pair over loopback (M4), apply
  a concurrent op to the same line, and assert the banner + review sheet appear with M4's three
  variants reachable.

## Placement / dependencies

- New: `apps/desktop/playwright.config.ts`, `apps/desktop/e2e/{fixtures.ts,popover,save,detail,
  notes-create,breadcrumb,conflict}.spec.ts`.
- Depends on: popover (41), detail (42), conflict banner/sheet (43), M4 pairing + M5 lazy creation
  already in the daemon. New dev deps: `@playwright/test` (+ `tauri-driver` if the WebDriver route
  is chosen) — human sign-off + `deny.toml` pass.

## Edge cases & invariants

- Each test runs against a **fresh** seeded workspace; no test may depend on another's side
  effects (no shared fixtures between slices, per constitution §7).
- The suite must be fast enough for the gate — keep it to these six scenarios, each under a hard
  per-test timeout, and reuse one daemon spawn per scenario (not per assertion).
- The "file changes only that line" assertion must include the trailing newline handling (plan
  §2.4 rule 7: `\r\n` preserved) so a byte-diff doesn't false-positive on line-ending rewrite.
- Conflict injection must be deterministic (a seeded op, not a sleep-and-pray) so the test is not
  flaky.

## Acceptance

- All six scenarios pass against a fresh seeded workspace, wired into the gate and CI, green on
  push (constitution §5: gate runs slice tests; CI is law).

## References

- Plan M7 acceptance · plan §3.2 interactions · plan §3.2.4 lazy creation.
- https://playwright.dev/ · https://v2.tauri.app/develop/tests/webdriver/

## As built (2026-09-13, agent)

Built from scratch this session. Chose neither of the notes' two named options outright
(`tauri-driver`/WebDriver, or "a thin test bridge that calls the daemon's gRPC the same way
`commands.rs` does") but a variant of the second, adapted for what a browser can actually do:

### Harness: a real daemon behind a real HTTP bridge, driven by a plain browser

`tauri-driver` was ruled out deliberately: it launches an actual native OS window, which is exactly
the "don't launch/drive the app to look at it" activity this repo's CLAUDE.md reserves for a human,
even without a screenshot involved — and it needs `safaridriver`/a real GUI session, which isn't
something to spin up unprompted. A **plain browser calling the daemon's gRPC the way `commands.rs`
does** was the other option, but a browser cannot speak tonic/gRPC directly (no unix sockets, no
HTTP/2 trailers) — so this reuses `commands.rs`'s exact logic (not a second implementation of it)
restated over plain JSON/HTTP:

- `apps/desktop/src-tauri/src/bin/e2e_bridge.rs` (new binary, feature-gated) — calls
  `desktop_lib::daemon::{ensure_daemon, DaemonClient}` exactly like `lib.rs::run()` does, and reuses
  `desktop_lib::dto::*`'s existing `From<pb::...>` conversions directly (made `dto` `pub` for
  this), for `list_files`/`get_file`/`apply`/`history`/`list_conflicts`/`resolve`/`get_notes`/
  `edit_notes` plus one test-only `debug_raise_conflict` (see conflict.spec.ts below). **Guarded so
  it can never ship enabled to production**: it only exists behind the `e2e-bridge` Cargo feature
  (`required-features` on its `[[bin]]`), which a plain `cargo build -p desktop` never enables —
  confirmed by grep-diffing `cargo build -p desktop`'s output with and without the feature.
- `apps/desktop/e2e/shim/{core,event,window}.ts` — stand-ins for `@tauri-apps/api/{core,event,
  window}`, active only under `vite dev --mode e2e` (`vite.config.ts`'s new mode-conditional
  alias). `core.ts` forwards `invoke()` to the bridge over `fetch`; `window.ts` reports the main
  window's label (quick-add isn't one of these six scenarios); `event.ts` is the one genuine
  compromise — see below.
- `apps/desktop/e2e/fixtures.ts` — `spawnDaemon(fixture)` builds `e2e_bridge`+`txtodod` once,
  seeds a fresh tempdir per named fixture, spawns the bridge (with `target/debug` prepended to
  `PATH` so `DesktopConfig.daemon_bin`'s default PATH-resolution finds `txtodod`), and waits on
  `/health`. `dispose()` kills the bridge and reads the daemon's own pidfile
  (`.txtodo/txtodod.pid`, the same file `apps/desktop/src-tauri/tests/support::wait_for_pid` reads)
  to kill it too, since killing the bridge alone doesn't reap its child.

**Why `Watch` isn't real here**: `e2e_bridge` intentionally doesn't implement the `Watch` stream —
`event.ts`'s `listen("daemon-change", ...)` instead polls `get_file`/`list_conflicts` for whatever
paths the app called `watch()` on and synthesizes a `Change` event when a hash or the conflict-id
set changes. This is still every byte and every flag coming from a real `txtodod` acting on a real
file — only the daemon→browser *transport* is polling instead of a push stream, invisible to
`$lib/daemon.ts`'s `onDaemonChange` callback either way. Building the real stream over HTTP (SSE)
was judged not worth it for six scenarios that already poll-and-retry via Playwright's own
`expect.poll`/auto-waiting assertions.

### The six scenarios

All in `apps/desktop/e2e/*.spec.ts`, `npm run test:e2e` (`playwright test`) to run, all currently
green (`14 passed` under `--repeat-each=2`, no observed flakiness after fixing two real bugs this
session's own dogfooding surfaced — see below):

| File | What it proves |
|---|---|
| `popover.spec.ts` | click → popover has the raw line, `id:` included, hidden in the main view |
| `save.spec.ts` | Enter saves; a byte-diff of the tempdir file shows **exactly one** changed line |
| `detail.spec.ts` | double-click → pinned parent + working notes editor + rendered sub-list |
| `notes-create.spec.ts` | opening detail alone writes nothing; the first keystroke creates the `ref:` dir + `notes.md`, and the parent line gains a `ref:` tag, in one op |
| `breadcrumb.spec.ts` | double-clicking a sub-list line nests the breadcrumb and re-pins the parent |
| `conflict.spec.ts` | the banner + sheet appear and all three resolutions are reachable; "keep mine" clears the flag and writes the text back |

### Two real bugs this harness itself found (not injected, not hypothetical)

1. **Hover-then-click races the pencil out from under itself.** `FileView.svelte`'s hover pencil
   only exists while `hoveredLine` is truthy, which CM6's own `mousemove` handler recomputes on
   every real pointer move — including the intermediate moves Playwright's `locator.click()`
   synthesizes while walking the mouse to the target, which can (and reproducibly did) make the
   pencil vanish mid-click. Worked around on the *test* side, not the app: `openPopoverFor` in
   `e2e/helpers.ts` does `hover()` then `pencil.dispatchEvent("click")` — a direct DOM click with no
   further synthetic pointer movement. Not a change to the shipped app; flagging in case a future
   redesign wants to make the pencil itself more click-robust (e.g. a wider hit target, or hover
   state that doesn't recompute for sub-pixel moves).
2. **`ReviewFlagDto.mine`/`.theirs` must carry the task's `id:` tag, not just its words** —
   discovered by hitting the daemon's own "inserted line does not carry id ..." refusal, then
   confirmed against `crates/txtodo-daemon/tests/grpc.rs::raise_flag`'s fixture shape. Recorded on
   `desktop-conflict-review`'s "As built"; `e2e/fixtures.ts::CONFLICT_MINE`/`CONFLICT_THEIRS` follow
   it.

### conflict.spec.ts's flag: raised directly in the store, not via a second daemon

The notes' acceptance wording ("spawn a second daemon... pair over loopback... apply a concurrent
op") is **not reachable today**: real daemon-to-daemon sync has no transport wired up at all yet.
Confirmed by research, not assumption — `crates/txtodo-daemon/src/pairing_grpc.rs`'s own doc
comment says outright "the leg that actually crosses between two daemons... has no transport yet";
`todo.txt` already tracks the acceptance test this would need as blocked
(`ref:sync-loopback-converge`); and the daemon's *own* integration tests hit the same wall and use
the identical substitute this suite uses:
`crates/txtodo-daemon/tests/grpc.rs::raise_flag` raises a `needs_review` flag by opening a second
connection to `.txtodo/oplog.db` directly ("what an import merge would do... no actual sync is
needed"). `e2e_bridge.rs::cmd_debug_raise_conflict` does the same thing, reusing `txtodo-store`'s
own `Store::open`/`ReviewRow`/`raise_flag` (new optional workspace-member dependencies, gated
behind the same `e2e-bridge` feature — not new external crates). Everything downstream of the flag
(`ListConflicts`, the banner, the sheet, `ResolveConflict`, the on-disk write) is the real
production path; only "how the flag got raised" is substituted, by the same mechanism the daemon
team already relies on. Flagged here rather than silently claiming full acceptance-criteria
coverage: **the real second-daemon scenario the notes describe cannot be tested until
`sync-loopback-converge` lands**, which is squarely a `crdt`/`sync` milestone concern, not a gap in
this Playwright suite.

### CI

Not wired into `.github/workflows/ci.yml` this session — same frozen-path/"ask, never silent" call
as `desktop-lezer-grammar`'s CI gate; see that task's notes for the proposed job shape (a Node
setup step is the shared prerequisite for both). `npm run test:e2e` is the manual/local entry point
until a human signs off on adding Node to CI.

## What to open and look at

- `cd apps/desktop && npx playwright install chromium` (once), then `npm run test:e2e` — expect
  `7 passed` (or `14 passed` with `--repeat-each=2`). This *is* the verification for this task; the
  six scenarios above are exactly what a human would otherwise click through by hand.
- If a test fails, `npx playwright test <file> --reporter=line` and read the printed DOM/error
  context (no screenshots are captured or needed — every assertion here is text/attribute-based).
