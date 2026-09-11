# Conflict banner and review sheet for needs_review (plan M7, design §4.7)

## Goal

When the daemon's `Watch` stream carries a task flagged `needs_review`, the desktop shows a conflict
banner (pending count) and a review sheet offering the three resolutions `mine` / `theirs` /
`merged` (plan M4, design §4.2). The user-visible guarantee is design §4.7: txtodo never silently
loses something you typed — so the UI *offers*, the human *picks*.

## Design

`needs_review` is produced by the M4 same-word-edit detection: after a merge, two concurrent
`EditText` ops overlapping in range mark the task — a **local flag in the op log, not the file** —
exposed via `Watch`. `txtodo conflicts` lists them; `txtodo conflicts resolve <line>
mine|theirs|merged` clears the flag by writing the chosen text as a new op (plan M4). The sheet is
the GUI for that command.

```ts
// apps/desktop/src/lib/types.ts
export interface ConflictEntry {
  line: number;                 // current line number
  task_id: string;              // id: ULID
  mine: string;                 // our device's text
  theirs: string;               // the peer's concurrent text
  merged: string;               // char-level CRDT interleave preview (design §4.2)
}
export type ResolveChoice = "mine" | "theirs" | "merged";
```

```svelte
<!-- apps/desktop/src/lib/components/ConflictReviewSheet.svelte -->
<section role="dialog" aria-modal="true">   <!-- traps focus, Esc closes (plan §3.3) -->
  <p class="task">{entry.merged}</p>
  <DiffView a={entry.mine} b={entry.theirs}/>       <!-- core diff_text, WASM -->
  <button on:click={() => resolve("mine")}>keep mine</button>
  <button on:click={() => resolve("theirs")}>keep theirs</button>
  <button on:click={() => resolve("merged")}>keep merged</button>
</section>
```

- **Char-level diff**: `core::diff_text` (design §3 `diff.rs`), shipped to the UI via the
  `txtodo-ffi` wasm target, renders the overlapping edit between `mine` and `theirs` so the human
  sees exactly what merged (design §4.2: "clients show a one-tap keep mine / keep theirs / keep
  merged").
- **Resolution** calls `Apply(Edit)` with the chosen text — the *same* path as the edit popover, so
  attribution, sync, and history are automatic. The op clears the flag (plan M4).
- **Banner**: a count of pending `needs_review` lines; clicking opens the sheet. Dismissing the
  banner must **not** clear the flag — clearing is a resolve action only, and only via a new op
  (same spirit as §3.2.6: no silent decisions).
- **Resurrect state** (design §4.7 `delete | edit → edit wins, task resurrected`): the sheet shows
  "resurrected by a concurrent edit" when the task was tombstoned on our side and edited on theirs.

## Placement/dependencies

- New `apps/desktop/src/lib/components/ConflictBanner.svelte` + `ConflictReviewSheet.svelte` +
  `DiffView.svelte`; the app store gains `pendingConflicts: ConflictEntry[]` fed from the `Watch`
  stream (Tauri event bridge — see [desktop-tauri-shell](../desktop-tauri-shell/notes.md)).
- Depends on M4 daemon (`needs_review` in `Watch`, `Apply` resolve path) and the `txtodo-ffi` wasm
  `diff_text` binding. No new crate/dependency.

## Edge cases & invariants

- The flag is per-op-log, not per-file: re-rendering or `Checkout` does not clear it; only a
  resolve op does.
- `merged` is a preview — it is never written until the human taps "keep merged".
- Banner count and the sheet's entry must agree with `txtodo conflicts` (same source of truth).
- Invariant (assert the negative): dismissing the banner or navigating away never clears a flag.

## Acceptance

- Conflict sheet appears when the test injects concurrent ops via a second daemon (plan M7).
- Resolving each of `mine`/`theirs`/`merged` writes the chosen line, clears the flag, and drops the
  entry from the banner count.

## References

- plan M4 (same-word detection, `txtodo conflicts`), M7 acceptance; design §4.7 (conflict table),
  §4.2 (CRDT), §3 `diff.rs`.
- [desktop-tauri-shell](../desktop-tauri-shell/notes.md) · M4 (txtodo-implementation-plan.md).
