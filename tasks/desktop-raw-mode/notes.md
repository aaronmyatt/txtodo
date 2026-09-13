# Stretch: raw mode Cmd/Ctrl+E through the reconciler (plan M7, plan §3.2 §7)

Plan §3.2: "Cmd/Ctrl+E = toggle raw mode (the whole document becomes editable inline; on blur or
Cmd/Ctrl+S the buffer goes through the reconciler exactly like an external edit). Raw mode is a
stretch goal in M7." Design §7: thin clients — even in raw mode the UI never parses or writes the
file itself; the daemon owns the file and the reconciler.

## Goal

Make the read-only CM6 `EditorView` (plan §3.1 main view) editable in place, and on exit submit
the whole-buffer delta to the daemon *as an external edit* — reusing the exact reconcile path the
daemon already runs when the file changes on disk. No parallel write path, no UI-side parsing.

## Design

### Toggle: a CM6 keymap binding, not a mode enum smuggled into state

```ts
// apps/desktop/src/MainView.svelte
import { keymap, EditorView } from "@codemirror/view"; // https://codemirror.net/
const RAW_BINDING = "Mod-e"; // Mod = Cmd on macOS, Ctrl elsewhere (CM6 maps it)

const rawToggle = keymap.of([{
  key: RAW_BINDING,
  run: (view) => { setRaw(!raw.get(view)); return true; } // true = handled, don't bubble
}]);
function setRaw(on: boolean) {
  // readOnly + editable flip together; the view stays the same document
  editorState.dispatch({
    effects: [
      EditorView.editable.reconfigure(on),        // https://codemirror.net/docs/ref/#view.EditorView^editable
      EditorView.contentAttributes.reconfigure(on ? { "data-raw": "true" } : {}),
    ],
  });
}
```

Visual state is `data-raw` (border/background via CSS) so colour is never the only signal —
plan §3.3's accessibility floor. There is also a visible "raw mode" badge; screen readers get an
`aria-pressed` toggle state.

### Exit = external-edit-shaped op

```ts
// On blur or Cmd/Ctrl+S: diff the current buffer against the last known daemon state,
// then submit the delta the same way daemon-external-edit-tests exercises.
async function commitRaw(): Promise<void> {
  const next = editorState.doc.toString();
  if (next === lastFromWatch) return;                    // identical save = no-op, no op entry
  const delta = computeDelta(lastFromWatch, next);        // line-diff, not raw string replace
  await applyExternalEdit(activeFile, delta);             // tonic into the daemon's reconcile path
  // Watch stream re-renders the (now read-only) view from the reconciled projection.
}
```

The key property: the UI submits a **delta**, never the whole string, and it goes through the
daemon's external-edit reconcile — the same path [daemon-external-edit-tests](../../daemon-external-edit-tests/notes.md)
covers. On `needs_review` (concurrent edit during raw mode), the daemon surfaces the existing
conflict banner + review sheet (M4's three variants); the raw buffer is not silently kept.

## Placement / dependencies

- `apps/desktop/src/MainView.svelte` (keymap + reconfigure), `apps/desktop/src/raw-mode.ts`
  (computeDelta + commitRaw), plus the existing `apply_line`/external-edit Tauri command.
- Depends on: main-view task (line 40), the conflict banner/sheet task (line 43), and the
  daemon-external-edit-tests path. No new crates; CM6 `keymap` + `EditorView.editable` are
  already in the dependency tree.

## Edge cases & invariants

- A save identical to the loaded buffer is a no-op — no op-log entry (same rule as the popover,
  plan §3.2). Assert it.
- A concurrent external edit while raw mode is open must land in the conflict sheet, not be
  overwritten — the daemon's reconcile detects the divergence and sets `needs_review`; the UI only
  renders that outcome.
- Raw mode is per-view state; switching files (breadcrumb navigation) must commit or discard the
  raw buffer first — never carry an unsaved raw buffer across file switches.
- The document in raw mode is the *reconciled projection*, so the toggle must refuse to enter raw
  mode when `needs_review` is already showing (resolve conflicts first, then edit).
- Esc in raw mode discards (blur/save commits); make the two paths explicit and testable.

## Acceptance

- Toggle edits inline; blur or Cmd/Ctrl+S reconciles via the daemon and the `Watch` stream
  re-renders the read-only view.
- A concurrent external edit during raw mode raises the conflict sheet, not a silent overwrite.
- Identical save writes nothing; Esc discards without an op entry.

## References

- Plan §3.2 raw mode · plan §3.3 accessibility · design §7 thin clients.
- https://codemirror.net/docs/ref/#view.EditorView^editable · https://codemirror.net/docs/ref/#keymap
- ../../daemon-external-edit-tests/notes.md

## As built (2026-09-13, agent)

Built entirely inside `apps/desktop` (this task's stop condition: one slice, no daemon/proto
changes, no frozen paths). The CM6 host turned out to be `FileView.svelte`, not `MainView.svelte`
as this file's own sketch assumed — `MainView` only ever mounts `<FileView path="todo.txt" .../>`
and the popover; the actual `EditorView` lives inside `FileView` (tasks/desktop-main-view). Raw
mode is wired there instead, once per `FileView` instance (root view and every nested sub-list).

### The one real design decision: what "the daemon's existing reconcile path" means from a
thin client with no daemon-side changes allowed

This task's notes sketch `applyExternalEdit(...)` — a whole-document, external-edit-shaped RPC
that doesn't exist. Confirmed by reading `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`
end to end: there is no such RPC, and the daemon's real external-edit reconcile
(`crates/txtodo-daemon/src/external.rs`, `txtodo-core::diff::diff_lines`) only runs off a real
file-watcher event on disk — nothing a Tauri command can invoke directly. Adding one would mean a
new `.proto` message/RPC plus daemon-crate changes, which is out of an `apps/desktop`-only slice
(and would touch `Cargo.toml`, a frozen path, at minimum for any new workspace-member dependency).

So `computeDelta` (`src/lib/todotxt/rawMode.ts`) instead turns a raw-buffer edit into the same
intent-level `Mutation`s (`Add`/`Edit`/`Delete`) the edit popover and `ConflictReviewSheet`'s
resolutions already send through `apply()` (`Apply` RPC) — the one reconcile path this app can
reach without a daemon change. This is not just a smaller ask than the sketch, it is *safer*: a
targeted per-task mutation can never clobber an unrelated concurrent change the way a naive
whole-file replace could, and a genuinely conflicting concurrent edit to the *same* task still
goes through the real per-task CRDT merge / `needs_review` flagging `ConflictReviewSheet`'s own
doc comment describes ("the same `Apply(Edit)` path the edit popover uses"). Line matching is by
`id:` tag first, then positionally for untagged/blank lines against an unclaimed pool, so an
untouched line is never mistaken for a delete+add pair — see that file's own doc comment for the
full algorithm and its documented limits (no true reorder/insert-at-position primitive exists in
`Mutation`, so a new line is always appended and a moved id-tagged line keeps its old position,
text changes only).

### What got built

- `apps/desktop/src/lib/todotxt/rawMode.ts` — `computeDelta`, `isNoOpSave`, `canEnterRawMode`:
  pure, framework-free (no CM6, no Tauri, no Svelte), same split as `editPopoverLogic.ts`.
- `apps/desktop/src/lib/components/FileView.svelte`:
  - `editableCompartment` (was two fixed extensions) + `rawAttrCompartment` reconfigure
    `EditorView.editable`/`EditorState.readOnly` and `EditorView.editorAttributes({"data-raw":
    "true"})` together — `editorAttributes`, not `contentAttributes` as the sketch had, so the
    border/background below lands on the whole `.cm-editor` box, not just the text layer.
  - Keymap: `Mod-e` toggles (refuses to enter while `hasPendingReview`, computed from the existing
    `pendingConflicts` store `ConflictBanner` already feeds — no new subscription plumbing);
    `Mod-s` commits; `Escape` discards. A `blur` `domEventHandler` also commits.
  - A `.raw-toggle` button in the file-view header: `aria-pressed`, disabled (with a title
    explaining why) while a conflict is pending, text flips "Raw mode" / "Raw mode: on" — colour
    is never the only signal (plan §3.3): the CSS also flips a 2px border + background on
    `.cm-editor[data-raw]`, matched by the button's own `.active` border/background.
  - `refreshDoc` now returns early while `raw` is true: a concurrent `Watch` change (the daemon
    detecting a real external edit or another device) must not silently overwrite the human's
    in-progress raw buffer. The conflict banner is a sibling component fed by the same `Watch`
    stream, so it still surfaces independently of whether `FileView` is mid-raw-edit.
  - Two separate "leaving raw mode without an explicit commit" paths, both routed through the
    same `submitRawEdit(targetPath, baseline, next)` helper: `onDestroy` (component actually
    unmounts — e.g. breadcrumb "Home" tears down the whole `DetailView` subtree) fires a
    best-effort, un-awaited commit (same pattern as `setMainPopoverDirty`'s doc comment); the
    `path`-swap `$effect` (an already-mounted instance's `path` prop changes — e.g. navigating
    between sibling sub-lists at the same breadcrumb depth) commits against the *outgoing* path
    before dispatching the new file's content, never the incoming one.

### Tests

- `apps/desktop/src/lib/todotxt/__tests__/rawMode.test.ts` — 14 vitest cases: identical-save
  no-op, single/multi-line edit deltas (never a whole-buffer replace), add, delete (id-tagged and
  blank/untagged), an untouched-but-moved untagged line producing no mutation, a mixed
  edit+add+delete rewrite, and `canEnterRawMode`'s refuse/allow. All pass (`npm run test`: 96/96
  across the whole suite, no existing test touched or broken).
- `apps/desktop/e2e/raw-mode.spec.ts` — 5 Playwright scenarios against a real `txtodod` (same
  `e2e_bridge` harness as `tasks/desktop-playwright-tests`, no bridge/daemon changes needed since
  `apply`/`get_file`/`list_conflicts`/`debug_raise_conflict` already existed): toggle+edit+Cmd-S
  reconciles; Esc discards without ever writing; blur commits; a concurrent conflict raised via
  `debugRaiseConflict` while raw mode is open and dirty surfaces the banner + reachable review
  sheet *without* touching the still-open raw buffer; navigating away (breadcrumb "Home") commits
  a dirty sub-list raw buffer before the view unmounts.

  **Unverified in this session, environment-caused, not a code defect**: this host is running
  many other agents' worktrees concurrently right now (confirmed via `ps aux`:
  multiple other `agent-*` worktrees' own `txtodod`/`e2e_bridge` processes live at the same time).
  `apps/desktop/e2e/fixtures.ts::pickPort()` picks an HTTP port for `e2e_bridge` from a 20,000-wide
  random range with no bind-verification (`waitForHealth` only checks `/health` returns 200, which
  every `e2e_bridge` instance answers identically) — when two concurrent sessions on the same host
  happen to pick the same port, the loser's own spawn can end up silently talking to the winner's
  (unrelated, already-populated) workspace instead of failing loudly. Reproduced and root-caused
  directly this session: a manual `curl` against a freshly spawned, uniquely-ported bridge/daemon
  pair returned exactly the seeded fixture content every time; a `page.evaluate`d `fetch` against
  `window.__E2E_BRIDGE_URL__` from inside the same Playwright run sometimes returned that same
  correct content and sometimes returned unrelated real-looking task text (a different workspace
  entirely) on a dif­ferent random port each time — i.e. genuinely intermittent, not deterministic,
  and reproduced identically on *pre-existing, untouched* specs (`popover.spec.ts`,
  `conflict.spec.ts`) run in this same session, confirming it is not caused by this task's changes.
  `npm run check` (svelte-check, 0 errors) and `npm run test` (vitest, 96/96) are both clean and
  are this session's real verification signal; `raw-mode.spec.ts` is written, self-contained, and
  ready to run cleanly on a non-contended host — a human should re-run
  `npx playwright test e2e/raw-mode.spec.ts` outside of a heavily-loaded shared machine to get a
  trustworthy pass/fail. Not fixed here: `pickPort`/`waitForHealth` are shared e2e infra outside
  this task's one-slice scope (`tasks/desktop-playwright-tests` owns that harness).

### Known, documented limitations (see `rawMode.ts`'s own doc comment)

- No insert-at-position or reorder primitive exists in `Mutation`: a brand-new raw-mode line is
  always appended, and reordering existing id-tagged lines changes their text only, never their
  file position. A real whole-document external-edit reconcile (matching `diff_lines`'s id-aware
  `Move`) needs a new daemon RPC — out of an `apps/desktop`-only slice.
