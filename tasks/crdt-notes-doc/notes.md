# `notes.md` as a Loro text doc, `NotesEdit` ops, gRPC `GetNotes`/`EditNotes` (plan M5)

Depends on [crdt-loro-doc](../crdt-loro-doc/notes.md). `OpKind::NotesEdit { file, edits }` already
exists in the model, marked `(M5)` (`crates/txtodo-model/src/op.rs`).

## This task is also a bug fix, and the bug is live on `main`

The walker has shipped `notes.md` in `DOCUMENT_NAMES` since M3
(`crates/txtodo-daemon/src/walker.rs:11`), and the actor builds a `DocState` for **every**
discovered document (`actor.rs:78`) — `DocState` being the *task* document model. So the daemon
currently parses prose as todo.txt lines and the reconciler mints an `id:` for each one.

Reproduced on `main` (2026-09-12), daemon built from `2c7336d`:

```
$ printf 'Some prose about the roadmap.\n\n- a bullet\n' > notes.md
$ txtodod --dir .
txtodod ready: 2 document(s), ...
$ # touch notes.md, wait for the watcher
$ cat notes.md
Some prose about the roadmap. EDITED id:01M28Q5ANKF80A9DKKGEHR64MZ

- a bullet id:01M28Q5ANKKFEGTMP24FGBVJ80
```

That is silent corruption of a user's prose file, and `undo` is the only way back. It should be
fixed *before* the M5 feature work — raised as its own todo.txt line rather than buried in this
task, because the fix (refuse to treat `notes.md` as a task document) is small, independent, and
wanted whether or not M5 ever lands.

## The two document kinds

`FilePath` currently carries no kind, and everything downstream assumes tasks. Make the kind
explicit and non-inferable-by-accident:

```rust
pub enum DocKind { Tasks, Notes }   // from the file name, decided once, at the boundary
```

"Parse, don't validate" (CLAUDE.md §3): decide the kind once when the walker yields a path, and
have the actor hold a `Doc` enum rather than a `DocState`. Then `DocState::from_file` can never be
reached with a notes path, which is the actual guarantee — not a check sprinkled at call sites.

## Notes state

- No lines, no ids, no blanks-as-entries. A notes document is a single `LoroText` plus the file's
  byte-fidelity bits (BOM, line ending, trailing newline) — the same fidelity contract `DocState`
  already honours, since round-tripping the user's bytes is the project's core promise.
- External-edit reconciliation uses `core::diff_text` (char-level, shipped at M1
  `tasks/core-diff`), not `diff_lines`. That is exactly the `TextEdit` shape `NotesEdit` carries,
  so the mapping is direct.
- Size bound: `MAX_NOTES_BYTES`, asserted. A notes file is user-authored prose with no line cap,
  and "as big as the disk" is not a bound.

## gRPC

`GetNotes` / `EditNotes` in `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`, new field numbers
only, regen check green (`tasks/proto-grpc`).

`GetFile` already exists and returns bytes; adding notes-specific rpcs rather than reusing it is
worth a sentence of justification in the proto comment — the reason is that `EditNotes` takes
`TextEdit`s against a known base version, where `Apply` takes ops against a task document. Two
shapes, two rpcs.

## Tests

- **Regression, first**: a workspace containing `notes.md` leaves it byte-identical after the daemon
  starts, watches, and sees an external edit. This test fails on `main` today.
- Round-trip: external prose edit → `NotesEdit` ops → materialise → byte-identical to the edit.
- BOM, CRLF, and missing-trailing-newline notes files survive unchanged, like the task-file cases in
  `tasks/core-parse-file`.
- A `notes.md` that happens to look like todo.txt (`x 2026-01-01 something`) is still treated as
  prose — the kind comes from the name, never from the content.
- `EditNotes` against a stale base version is rejected with a typed error, not silently applied.

## As built (2026-09-13, verified/documented — implementation landed earlier, undocumented)

Implemented in `crates/txtodo-daemon/src/notes_actor.rs`, `notes_state.rs`, `notes_mirror.rs`,
`notes_registry.rs`, `notes_history.rs`; `GetNotes`/`EditNotes` wired in `crates/txtodo-proto`. All
tests pass (`cargo test -p txtodo-daemon`), including:

- **The regression bug, fixed and guarded**: `workspace_tests::notes_md_is_left_alone`. The fix is
  exactly this file's prescription — `DocKind` decided once at the walker boundary, `NotesActor`/
  `NotesState` hold no `DocState`, so a notes path can never reach the task parser.
- `round_trips_bom_crlf_and_missing_trailing_newline`, `empty_bytes_round_trip_to_empty_bytes`,
  `reapplying_the_same_text_back_is_a_true_no_op` — byte-fidelity contract.
- `notes_actor_tests::concurrent_edits_to_different_parts_of_the_text_merge_without_data_loss` —
  round-trip via `NotesEdit` ops through the Loro mirror.
- `notes_grpc.rs`: `get_notes_before_any_edit_is_not_an_error`,
  `edit_notes_lazily_creates_the_ref_dir_and_get_notes_returns_it` — gRPC surface.
- `MAX_NOTES_BYTES` bound asserted in `notes_state.rs`.

**Deviation from this file's own checklist, found and left as-is rather than silently "fixed"**:
`EditNotes` (`NotesEditRequest { task, new_text }` in the proto) carries no base-version field at
all, and `NotesActor::edit` does not reject anything as stale — it diffs `new_text` against
whatever the current server-side text is (`txtodo_core::diff_text`) and applies that as one
`NotesEdit`, on the stated reasoning (see `notes_actor.rs`'s doc comment on `edit()`) that this lets
two devices' concurrent edits merge character-wise once both land in the mirror, the same way
`import_updates` merges a peer's Loro updates. That is a real, considered design choice, but it is
the opposite of "reject a stale base version with a typed error" — it never rejects, it always
merges. I did not change this: reworking it to add optimistic-concurrency rejection would mean a
proto field change and every caller (CLI, and the M6 MCP `todo_notes_set` tool once built) passing a
version through, which is more than a docs-catch-up pass should decide unilaterally. **Flagging for
a human call**: keep the current diff-and-merge behavior (drop this checklist line, like
`crdt-conflict-table`'s configurability question), or add real staleness rejection.

No other gaps found. `todo.txt` line marked done — the checklist item above is tracked here, not
silently dropped.
