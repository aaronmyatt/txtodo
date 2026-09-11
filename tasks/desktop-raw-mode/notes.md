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
