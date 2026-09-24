# Visual regression snapshots light and dark and 10k-line first paint ≤ 500 ms (plan M7, plan §3.1 §7)

Plan M7 acceptance: "Visual regression snapshots for light and dark themes. Startup to first
paint of a 10 k-line file ≤ 500 ms on the CI runner."

## Goal

Two deliverables: committed light/dark goldens for every themed surface, and a measured,
asserted first-paint budget for a 10 k-line file. Both are reproducible numbers on the CI runner,
not vibes.

## Design

### Snapshots: one Playwright project per theme

```ts
// apps/desktop/playwright.config.ts — extends the e2e config from the playwright task.
projects: [
  { name: "light", use: { colorScheme: "light", snapshotPathSuffix: "-light" } },
  { name: "dark",  use: { colorScheme: "dark",  snapshotPathSuffix: "-dark"  } },
],
```

Each themed surface asserts with `toHaveScreenshot` (https://playwright.dev/docs/test-snapshots),
against a **10 k-line fixture** (the same fixture the perf test uses — one source of truth):

- main view: `id:` tags hidden, ref indicators and `n/m` progress decorations visible, line
  numbers on, the "Add a line" trailing row present (plan §3.1).
- edit popover (plan §3.2): raw line visible, chips row, validation-error inline style.
- detail view: pinned parent, notes editor, breadcrumb, footer with directory path + sync status.
- conflict sheet: M4's three variants (mine / theirs / merged).

Goldens are committed and regenerated only via an explicit update command (`--update-snapshots`),
never silently. CI runs the snapshot tests **without** update and fails on any pixel drift.

### First paint of 10 k lines is a measurement, not a hope

CM6 virtualizes its viewport (https://codemirror.net/docs/ref/#view.Viewport), so drawing 10 k DOM
lines is not the risk — eagerly tokenizing or decorating all 10 k is. The fix is to bound
decorations to the visible range:

```ts
// apps/desktop/src/decorations.ts
// Provide decorations lazily over the viewport range only, so a 10 k-line file costs
// the visible window, not the whole document. Ref: https://codemirror.net/docs/ref/#view.Decoration
import { StateField, RangeSetBuilder } from "@codemirror/state";
export const lineDecorations = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update: (deco, tr) => {
    const builder = new RangeSetBuilder<Decoration>();
    for (const { from, to } of tr.state.visibleRanges) { decorateRange(builder, tr.state, from, to); }
    return builder.finish();
  },
  provide: f => EditorView.decorations.from(f),
});
```

The perf test loads the 10 k-line fixture, records time from daemon `Watch` first emit (or
`EditorView` mount) to first paint, and asserts ≤ 500 ms on the CI runner:

```ts
// apps/desktop/e2e/perf.spec.ts
test("10k-line first paint ≤ 500 ms", async ({ page }) => {
  await page.goto("/?file=todo-10k.txt");
  const started = Date.now();
  await page.waitForSelector("[data-line-count='10000']"); // first visible line rendered
  const paintMs = Date.now() - started;
  expect(paintMs).toBeLessThanOrEqual(500);
});
```

The ≤ 500 ms budget is a constitution number — it goes into `budgets.json` (as a perf note or a
new perf key) and gets a named check, never a comment-only promise.

## Placement / dependencies

- `apps/desktop/playwright.config.ts` (theme projects), `apps/desktop/e2e/visual/*.spec.ts`,
  `apps/desktop/e2e/perf.spec.ts`, `apps/desktop/src/decorations.ts` (viewport-bounded).
- Depends on: main view (40), popover (41), detail (42), conflict sheet (43), the 10 k-line
  fixture generator, and the Playwright task's daemon harness.

## Edge cases & invariants

- One fixture, two consumers: the 10 k-line file is generated once (a checked-in generator or a
  committed fixture) and shared by snapshots and perf, so the perf number and the goldens describe
  the same document.
- Snapshots are deterministic: disable animations/transitions, freeze the clock shown in the
  popover footer (`Line N · device, relative time`), and pin the seed — otherwise goldens drift on
  every run.
- The 500 ms assertion runs on the **same CI machine** as the snapshots so the number is
  reproducible; a one-off slow runner must fail the check, not be hand-waved.
- No silent golden regeneration in CI — an update is an explicit, reviewed commit.

## Acceptance

- Light and dark goldens committed for main view, popover, detail view, and conflict sheet; CI
  fails on drift without an explicit update.
- Perf test: 10 k-line first paint ≤ 500 ms on the CI runner, asserted and wired into the gate.

## References

- Plan M7 acceptance · plan §3.1 main view · plan §3.2 popover/detail.
- https://playwright.dev/docs/test-snapshots · https://codemirror.net/docs/ref/#view.Viewport
- https://codemirror.net/docs/ref/#state.StateField

## As built (2026-09-13, agent)

Built on top of `tasks/desktop-playwright-tests`' harness (real daemon + `e2e_bridge`, see that
task's notes) rather than replacing it. New: `apps/desktop/e2e/tenKFixture.ts`,
`apps/desktop/e2e/perf.spec.ts`, `apps/desktop/e2e/visual/{deterministic.ts,main-view,edit-popover,
detail-view,conflict-sheet}.spec.ts` + their committed `-snapshots/*.png` goldens. Small additive
changes: `apps/desktop/playwright.config.ts` (theme projects), `apps/desktop/e2e/fixtures.ts` (a
`"ten-k"` fixture), `apps/desktop/src/lib/components/FileView.svelte` (`data-line-count` testability
attribute), `apps/desktop/package.json` (`test:visual`/`test:visual:update`/`test:perf` scripts).

### A real, pre-existing bug this task's own dogfooding found: the e2e harness was silently mocked

Before any of this task's own code, running the *existing* six-scenario Playwright suite
(`desktop-playwright-tests`) failed almost entirely — `.cm-line` elements not found, `hover`/
`dblclick` timeouts, and one test's page showing "Ship the auth rewrite"/"release-notes2" content
that belongs to `src/lib/mock/state.ts`'s in-browser demo data, not to any seeded fixture. Root
cause, confirmed by reading the code, not guessed: `src/lib/tauriShim.ts::hasTauri()` checks
`"__TAURI_INTERNALS__" in window`, which is **never** true in a plain Chromium tab (no Tauri
runtime) — so `invoke()`/`listen()` always fell through to `mockInvoke`/`mockListen`, regardless of
`vite.config.ts`'s `mode === "e2e"` alias swapping `@tauri-apps/api/{core,event}` for the real
`e2e_bridge` shims. That alias is exactly what `tasks/desktop-playwright-tests`' whole design
depends on ("the code under test is the real reconciler, real file bytes, real op log") — with this
gate wrong, the harness was mocked the entire time, for every spec, not just this task's new ones.

Fixed with one additive line, guarded by `import.meta.env.MODE === "e2e"` (Vite's own build-time
mode string, https://vite.dev/guide/env-and-mode.html#modes) so nothing changes for the real Tauri
app or the plain `npm run dev` preview — see `tauriShim.ts`'s updated comment for the full
explanation. After the fix, all 8 functional-project tests (the original 6 scenarios + this task's
own perf test) pass reliably. This was necessary to deliver *any* verified Playwright output for
this task, so it's fixed here rather than only flagged — but since it silently affects the whole
existing suite (not just visual-regression/perf), it's called out prominently for whoever owns
`desktop-playwright-tests`/`tauriShim.ts` going forward.

Also added, small and additive: `e2e_bridge.rs`'s `invoke` gained a `"workspace_root"` case (it
already knows its own workspace path — no daemon RPC needed) and `e2e/shim/core.ts` now forwards
that command instead of hardcoding `""`. Without it `DetailView.svelte`'s footer
(`{absoluteRefDir}`) rendered empty, which the six original scenarios never needed but this task's
detail-view golden does (acceptance: "footer with directory path").

### Decorations were already viewport-bound — no change needed

Re-read `$lib/todotxt/decorations.ts` (from the earlier `desktop-main-view` task) before touching
anything: `idTagsHidden` uses CM6's `MatchDecorator` (inherently viewport-scoped by design) and
`lineDecorations` builds its `RangeSetBuilder` only over `view.visibleRanges`, already exactly the
pattern this task's notes sketch. Confirmed empirically too: the perf test's first-paint number
(**~230ms** locally, budget 500ms) would not hold for a 10k-line file if either were eager. Nothing
in `apps/desktop/src/lib/todotxt/decorations.ts` changed this session.

### The ≤500ms budget: hard-coded, not in budgets.json (frozen path)

`FIRST_PAINT_BUDGET_MS = 500` is a named constant in `e2e/perf.spec.ts` itself, with a comment
explaining why: `.claude/budgets.json` is a frozen path requiring an explicit human-reviewed
`/setup` write, called out in this task's brief as different from most frozen paths in this repo.
This session did not touch it, run `/setup`, or propose a diff for it. **Flagging for the
orchestrator**: fold this into the pending `.claude/budgets.json`/`.claude/stack.md` review already
underway for the related `desktop-stack-mapping` task — promote `FIRST_PAINT_BUDGET_MS` into a named
perf key there. `tasks/desktop-visual-regression/todo.txt`'s corresponding line is tagged `@human`
and left unchecked for the same reason.

### "M4's three variants (mine/theirs/merged)" is one render, not three fixtures

Read literally, "conflict sheet (all three variants: mine/theirs/merged)" could mean three separate
golden images. Checked against how every other task that uses this exact phrase means it
(`tasks/desktop-conflict-review/notes.md`: "a review sheet offering the three resolutions mine/
theirs/merged" — one sheet, one flag, three *buttons*; `desktop-playwright-tests/e2e/conflict.spec.ts`
asserts exactly that in one render) — it's one sheet showing the diff with all three resolution
buttons reachable, not three different conflict fixtures. One golden per theme
(`conflict-sheet.spec.ts`), matching the sheet's actual only visual shape.

### Determinism

- Animations/transitions: relied on Playwright's own `toHaveScreenshot` default
  (`animations: "disabled"`), which already freezes CSS animations at their initial frame —
  including CM6's `cm-blink` cursor-blink keyframe. No custom CSS injection needed; see
  `e2e/visual/deterministic.ts`'s doc comment for the full reasoning and a Playwright doc link.
- Relative-time clock (the popover's "Line N · device, 3m ago" footer): every fixture this task's
  specs use seeds `todo.txt` directly on disk (not through the daemon's op log), so `history()`
  finds zero ops for the task and the footer never renders a relative-time string in the first
  place — nothing to freeze. `e2e/visual/deterministic.ts` documents this and exports `FROZEN_NOW_MS`
  for a future golden that does need one.
- The `10k` fixture generator (`tenKFixture.ts`) has no randomness (no `Math.random`/`Date.now`) —
  byte-identical every call, so there's no "seed" to pin.
- One real non-determinism found and fixed: the detail-view golden's footer shows the real,
  per-test `mkdtempSync` tempdir path. Masked the whole `.detail-footer` block (not just the `.dir`
  span — masking only the span left a few boundary pixels leaking through from the span's width
  varying by the random tempdir characters' glyph widths in a proportional font). Verified stable
  goldens across five consecutive re-runs after the fix.

### What's verified here vs. what still needs a human/CI look

- **Verified in this sandbox**: `npm run check` (svelte-check, 0 errors), `npm run test` (vitest, 82
  passed), `npm run test:e2e` (Playwright, **16/16 passed** — 8 functional incl. perf, 4 visual
  specs × 2 themes), `cargo clippy -p desktop --features e2e-bridge --bin e2e_bridge -- -D warnings`
  (clean). Re-ran the full suite and the detail-view spec alone multiple times to confirm no drift.
- **NOT verified for CI / cross-platform — flagging explicitly, per this task's own gate
  instructions**:
  - All 8 committed PNGs are named `*-darwin.png` (Playwright's own platform-suffix convention) and
    were generated on this macOS sandbox using whatever system font actually resolved from the
    app's `Inter, Avenir, Helvetica, Arial, sans-serif` stack (Inter isn't installed here, so it's
    a fallback, not Inter). A Linux CI runner would look for `*-linux.png`, find nothing, and
    report "snapshot doesn't exist" — a failure, not a silent pass, but not a useful pixel
    comparison either. **A human needs to either regenerate goldens once on the actual CI OS/image
    (ideally Playwright's official Docker image, which pins font rendering:
    https://playwright.dev/docs/docker) or accept macOS as the only place these run.**
  - **The app has zero dark-mode CSS anywhere** (checked: no `prefers-color-scheme`, no
    `data-theme`, no `dark` token in any `src/**`) — confirmed by grep before writing a single
    spec. The `light`/`dark` Playwright projects and their `colorScheme` wiring are real and
    correct, but today's light and dark goldens for main-view/edit-popover/conflict-sheet are
    **byte-identical** (confirmed: same file size, diffing shows zero pixels) because nothing in
    the app actually responds to `colorScheme` yet. This is a deliberate scope boundary, not an
    oversight: styling a real dark theme across `MainView`/`FileView`/`EditPopover`/`DetailView`/
    `ConflictBanner`/`ConflictReviewSheet` is a cross-component design task of its own, well outside
    "visual-regression test infrastructure," and risks the parallel `desktop-raw-mode` task's
    `MainView.svelte` edits. The infrastructure is ready and will start actually differentiating
    the moment a future task adds real dark-mode CSS — no test changes needed then.

### Gate commands (for a human to re-run)

```
cd apps/desktop
npm run check          # svelte-check
npm run test           # vitest unit tests
npm run test:e2e       # playwright: functional (8) + visual light/dark (8) = 16
npm run test:perf      # perf.spec.ts alone
npm run test:visual        # light+dark only, no update
npm run test:visual:update # regenerate goldens explicitly — the ONLY sanctioned way, never CI
```

## Decision: goldens are local-only (2026-09-24, human)

The goldens are `-darwin.png`, taken and reviewed on a Mac; the nightly runs on ubuntu and had no
`-linux` set, so its visual projects could never pass. Dropped them from
`desktop-e2e-nightly.yml` (it now runs `--project=functional`, which still includes perf.spec.ts);
the visual check is `just goldens-check` / `goldens-update` / `goldens-review` on a Mac. Not filed:
Linux goldens. Revisit if the goldens ever need to gate a merge.
