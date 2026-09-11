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
