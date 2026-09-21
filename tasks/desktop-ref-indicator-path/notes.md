# desktop-ref-indicator-path

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

Every `n/m` ref progress indicator on the desktop main view is silently gone in a default-layout
workspace. This is a live regression from the layout work, not a styling nit.

## Cause

`lineDecorations()`'s first argument is `containingPath`, a *file* path — see the unit test at
`apps/desktop/src/lib/__tests__/lineInfo.test.ts:138`. `FileView.svelte:143,322,443` passes
`dirOf(path)`.

Under the old beside-the-list layout that happened to work: `resolveRefIndicator` did
`joinPath(dirOf(containingPath), slug)`, so a dir in, a dir out. The new `refDirFor`
(`src/lib/todotxt/lineInfo.ts:96`, `decorations.ts:149`) instead compares
`containingPath !== layout.todo_file`. `""` never equals `"todo.txt"`, so the root list takes the
*nested* branch: `ref:q1-goals` resolves to `q1-goals/todo.txt` instead of
`tasks/q1-goals/todo.txt`. Nested lists are wrong too — a double `dirOf` gives `tasks/<slug>`
instead of `tasks/q4/<slug>`.

Fix: pass `path` / `newPath` through unchanged.

## Fallout

- `apps/desktop/e2e/visual/main-view.spec.ts:23,37` — the `ten-k` fixture now seeds
  `tasks/q1-goals/todo.txt` (`e2e/fixtures.ts:118`), so `beforeEach`'s
  `expect(.cm-todotxt-ref-indicator).toBeVisible()` times out and both the light and dark projects
  fail. This landed green because the Playwright suite runs only in
  `desktop-e2e-nightly.yml`, not `ci.yml`. The goldens were also knowingly left un-regenerated when
  the layout line closed — `desktop-e2e-nightly.yml:49` runs the visual project without
  `--update-snapshots`, so the nightly has been red with no ticket naming it.
- `apps/desktop/src/lib/mock/tauriMock.ts:92` answers `workspace_layout` with `refs_dir: "tasks"`
  while `mock/state.ts:76,88` seeds `release-notes/todo.txt` beside the list. In browser/mock mode
  DetailView composes `tasks/release-notes/todo.txt`, which `get_file` rejects with
  `mock daemon: unknown path` (`tauriMock.ts:50`) — the sub-list detail view is dead in the dev
  mock. Either move the mock files under `tasks/` or return `refs_dir: "."`.

Order matters: fix FileView first, then re-run `npm run test:e2e`, then regenerate goldens. Do not
regenerate goldens against the broken render.

For the record, `refDirFor` itself matches `WorkspaceLayout::ref_dir_for` exactly, including the
`refs_dir = "."` case. The bug is entirely at the call site.

See [[layout-client-gaps]].
