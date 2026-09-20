# desktop-reorder-propagates

## Goal

Moving a line in the desktop editor (Alt+Up/Down, or cut and paste) must change the file, at once.
Today it does not: the file keeps the old order and the next repaint puts the line back.

## Why it fails today

- `FileView.svelte` saves a buffer with `rawMode.ts::computeDelta`: per-task `Edit`/`Add`/`Delete`.
- That delta has no way to say "this line moved" (see its own doc: "reordering ... changes their
  text only, never their position"). A moved line with the same text is a no-op, so nothing is
  sent, and `refreshDoc` repaints the daemon's order.
- Under Sidecar it is worse: a line has no `id:` tag, so an edited line matches nothing and
  becomes `Delete` + `Add`, and `Add` appends. An edit moves the line to the bottom.
- A save only happens on blur or Cmd-S, so even a working reorder would not be "at once".

## Design

- Save the buffer with `Mutation.Replace { base_hash, contents }` (proto field 7, daemon
  `replace.rs`): a whole-document compare-and-swap that the daemon reconciles like an external
  edit, so untouched lines keep their identity and a move is a move. The CLI already uses it.
- `base_hash` is the hash `GetFile` gave with the baseline text; `FileView` keeps it next to
  `baseline`.
- A refused `Replace` (`FAILED_PRECONDITION`: the document changed since the baseline) falls back
  to the old `computeDelta` batch against the old baseline. That path cannot move a line, but it
  only touches the lines the human changed, so a concurrent change is not lost. The reorder is
  then lost once, and the repaint shows it; the human moves the line again.
- "At once": a change that only reorders lines (same lines, new order) saves after a short
  debounce (300 ms) without waiting for blur. Typed text still waits for blur or Cmd-S, so a
  half-typed line never reaches the disk, the op log or a peer.
- Bridge: `MutationDto::Replace` in `src-tauri/src/dto.rs`, `{ kind: "replace" }` in `daemon.ts`,
  and the mock backend.

## Rejected

- `MoveBefore` per moved line: the buffer has no ids under Sidecar, so the client would have to
  diff lines by text and guess which one moved. The daemon's reconciler already does that.
- Save every keystroke: half-typed lines on disk and in every peer's op log.

## Known gaps

- The TUI has no reorder action at all, so there is nothing to propagate there. A `J`/`K` "move
  task down/up" over `MoveBefore` is a new feature; it is its own root line.

## As built (2026-09-20)

- Bridge `a3c8276`: `MutationDto::Replace`; `DaemonError::apply_text` starts a `FAILED_PRECONDITION`
  refusal with `failed-precondition:`. tonic 0.14 prints a code as prose, so there was nothing
  stable to match before. The e2e bridge answers the same text.
- Frontend `3bb7f54`: `todotxt/saveBuffer.ts` (`saveBuffer`, `isReorderOnly`, `matchEndings`,
  `isStaleBase`); `FileView.svelte` keeps `baselineHash`, saves through `saveBuffer`, and adopts
  the saved text as the baseline after a `Replace` (the reply carries the new hash), so a second
  quick move is not refused for naming the old hash.
- Daemon test: `tests/replace_apply.rs::sidecar_replace_with_moved_lines_keeps_each_lines_identity`
  reads `GetFile`'s `task_ids` before and after a reordering `Replace`: ids follow their lines.
- Side effect, wanted: under Sidecar an edited line used to become delete + append (it jumped to
  the bottom and lost its history). With `Replace` it stays where it is and keeps its id.

Still broken or not proven:

- Playwright was not run (it starts browsers; ask first). `e2e/inline-edit.spec.ts` line 43 says
  "a delta, not a whole-file replace"; the file on disk is the same either way, but nobody has
  watched that spec pass since the change.
- The 300 ms reorder save is wired in `FileView.svelte` with no test of the timer itself.
- Tagged mode: a save that adds a line gets its `id:` stamped by the daemon. If the human types on
  during that round trip, the next `Replace` sends the line without the tag and the daemon sees a
  new task. Sidecar, the default, has no tag to lose.
- The installed daemon must know `Replace` (it has since `437368b`); an older one answers
  `Unimplemented`/invalid, which is not a stale base, so the save fails loudly, not silently.

Check by hand (2 minutes):

1. Open a workspace in the desktop app, put the caret on a line, press Alt+Up.
2. Within a second `cat todo.txt` shows the new order; the line does not jump back.
3. Edit a word on a line, press Cmd-S: the line stays in place in the file.
