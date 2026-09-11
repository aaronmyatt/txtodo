# Move a line between files moves its `ref:` directory, with rollback — M5

`specs/ref-directories.md` rule 8. The spec is normative and mirror-checked, so behaviour comes
from the rule, not from this file.

## The op shape already exists; the behaviour is the work

`OpKind::Move { task, after, to_file }` has carried `to_file` since M3
(`crates/txtodo-model/src/op.rs`), and two seams return `Unsupported` today: `state.rs::move_task`
(`"cross-file Move"`) and `mutation.rs` `Mutation::Move` (`"Move between files"`). M5 removes both.
Do not invent a second op kind.

## One op, two documents — nail down how the destination learns of it

A single-writer actor owns one file, but a cross-file move is one `Op` whose `file` is the source
and whose `to_file` is the destination. Decide, once and in one place, how the destination actor
ingests it:

- The source actor authors the op (removes the line from its projection).
- The destination actor must apply an op whose `Op.file != self.path` but `to_file == self.path`,
  inserting the line after `after`, which resolves against the *destination's* ids.

That means the per-file replay/history path (`history.rs`, `recover`) must select ops by "this file
is the op's `file` **or** its `to_file`", not by `file` alone. Assert it in a test: a destination
actor hydrates a move it did not author.

## Rollback ordering — shared with rename, and that sharing is a duplication to flag

Rule 8: "If the move fails mid-way, the op is rolled back and the user is told." The directory move
and the op are two effects; a crash between them is unavoidable. Order them so the recoverable
failure is the harmless one, exactly as [daemon-ref-creation](../daemon-ref-creation/notes.md)
orders rename: **write the op first, move the directory second.** A crash leaves a dangling ref
(rule 9, recovers on next lazy write); the wrong order leaves an orphan directory (rule 10, needs
`prune`).

The rollback path (detect directory-move failure → remove the just-written op → report) is the same
shape as rename. Copy it; flag the third copy in `ABSTRACTIONS.md`, do not extract (constitution §2).

## The directory follows the *destination* file

Rule 8: "moves its directory to sit beside the destination file." The destination's directory may
differ from the source's, so the moved ref dir's new parent is computed from `to_file`, not from
the source path. The collision rule (rule 4) applies at the destination: a slug already taken gets
`-2`, `-3`. Reuse the exclusive-create / namespace-check logic from
[daemon-ref-creation](../daemon-ref-creation/notes.md), not a re-implementation.

## Edge cases

- `after` (new predecessor) is a task id in the *destination* document; resolving it against the
  source is a stale-id bug. State which document `after` names in the op's doc comment.
- Moving a line to its own file is a within-file move (already works); do not route it through the
  cross-file path.
- `Move` does not insert a blank in the source (that is `Delete { leave_blank }`, a different
  mutation); line-number stability is the client's call.

## Tests

- Move a line with a `ref:` dir between two files: line lands in the destination, the dir moves
  beside the destination, and a slug collision there gets the `-2` suffix.
- Destination actor hydrates a move whose `Op.file` is not its own path.
- Simulated directory-move failure: the op is rolled back and the source is byte-identical.
- Crash between op-write and dir-move leaves a dangling ref, not an orphan.
