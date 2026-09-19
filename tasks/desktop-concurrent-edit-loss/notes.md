# desktop-concurrent-edit-loss

## Goal

Lines typed in the desktop editor must never be repainted away, duplicated, or turned into deletes
of someone else's lines — whether the file changes underneath from an agent, a human in another
editor, another device, or the editor's own previous save.

Found by code reading only (2026-09-19); nothing here has been reproduced against a live daemon yet.

## Root causes (ranked)

1. **Phantom trailing line.** `rawMode.ts` `splitLines` uses `text.split("\n")`, so a file ending in
   `\n` gets a trailing `""`. The daemon has no such line (`txtodo-core/src/file.rs` `split_lines`).
   Typing on the "Add a line…" row makes `computeDelta` emit `add "new"` then
   `delete {line_number: N, task_id: ""}`. The id-less delete skips the daemon's stale guard
   (`mutation.rs` `resolve`), so it hits line N, which is now the task just added (or an agent's task
   if one was appended meanwhile). File bytes end up unchanged, nothing is written, the daemon still
   broadcasts a `Change`, and `refreshDoc` puts the old text back.
2. **`refreshDoc` races.** `FileView.svelte` checks `dirty` *before* `await getFile()`, so keystrokes
   typed during the round trip are overwritten. Refreshes also overlap and can finish out of order.
3. **Duplicate Watch streams.** `commands.rs` `watch` spawns a forwarder per call and never cancels
   it; `watch()` is called from FileView, ConflictBanner, DetailView and on every remount/path swap.
   One `Change` therefore fires several `refreshDoc` calls, widening race 2.
4. **`commit()` swallows errors.** It clears `dirty` first, and on Apply failure sets `loadError`
   then calls `refreshDoc()`, which wipes the buffer and clears `loadError`. Any rejected batch is
   silent data loss. Rejections seen in the code: `Add ""` (`NotATask`), `Delete` of a blank
   (`Blank`), a second delete in one batch (line numbers shift → `Stale`/`NoLine`).
5. **Stale `baseline`.** While dirty, `refreshDoc` returns without updating `baseline`, and nothing
   re-runs it after commit. The next save diffs against old line numbers/ids, so lines the daemon
   already tagged get added again, or the batch is rejected (cause 4). Cmd-S then keep typing hits
   the same thing with no agent involved.

Also noted: `Add` always appends (no insert-at-position), and there is no autosave — saves only
happen on blur, Cmd-S or unmount, so a crash loses the buffer.

## Not implicated

Daemon file watcher vs its own writes (`arm` before rename, hash check first) and the Loro mirror
(never decides bytes) looked correct.

## Repro to try first

Run `txtodo append`/an agent write in a loop against a file while typing on the last line in the
desktop app; then blur. Expect lost text within seconds; `txtodo log` should show `insert` then a
`deleted` set_field for the phantom-line case.

## Plan

Do sub-lines 1 → 4 in order; each is small and independently shippable. Line 1 fixes the
deterministic loss and may change the test at `rawMode.test.ts` ("emits Delete for a removed
untagged/blank line") which assumes the phantom line. Line 5 (baseline rebase) is a design call —
tagged `@human`: needs a decision on 3-way merge vs. "block the repaint and show a conflict banner".

## Open questions

- Rebase vs. banner for line 5 (above).
- Should the editor autosave (debounced) so a repaint has less to lose? Not in scope until 1–4 land.

## As built (2026-09-19)

Lines 1–4 done, commit 8551f3e.

- `rawMode.ts::splitLines`: drops the naive split's trailing `""` only when `text` itself ends in
  `\n` (not a blanket trim — `"a\n\n"` still keeps its real middle blank line), matching
  `txtodo-core/src/file.rs::split_lines`'s byte-iterator loop exactly, verified case by case
  including the `"\n"` → one blank line edge case. New regression test:
  `computeDelta("a\nb\n", "a\nb\nnew") === [{kind:"add", line:"new"}]`, no phantom delete.
- `FileView.refreshDoc`: re-checks `dirty` after the `getFile` await, plus a per-call `refreshSeq`
  counter so a response superseded by a newer call (not just a dirty buffer) is also discarded.
- `FileView.commit`: on `Apply` failure, restores `dirty = true` and sets `loadError` without
  calling `refreshDoc()` — the buffer keeps the human's rejected text instead of being silently
  replaced by the daemon's pre-edit content.
- `commands.rs::watch`: one shared stream per connection (`AppState::watch_started`, reset in
  `connect_and_store` on every reconnect) instead of one forwarder per call. The `paths` argument
  is gone entirely (server always watches everything now; every listener already filtered to its
  own path client-side) — updated `$lib/daemon.ts`, `FileView`/`DetailView`/`ConflictBanner`'s call
  sites, and the e2e shim (`core.ts`/`event.ts`), which now derives its polling-loop path set from
  `get_file`/`list_conflicts` calls instead of a `watch(paths)` argument that no longer carries any.

## Known gaps

- Line 5 (rebase baseline while dirty) is unbuilt, `@human`: needs a decision on 3-way merge vs. a
  "block the repaint, show a conflict banner" UX, not just an implementation.
- Nothing here has been driven against a real, running desktop app by a human yet — this pass is
  vitest (rawMode) + `cargo check`/`clippy -p desktop` + svelte-check only.
