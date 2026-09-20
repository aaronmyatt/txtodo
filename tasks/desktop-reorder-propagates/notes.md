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
