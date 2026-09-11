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
