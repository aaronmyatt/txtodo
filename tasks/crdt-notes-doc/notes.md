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
