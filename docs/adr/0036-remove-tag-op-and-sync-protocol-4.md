# 0036 — Remove a tag by name, not by offset; sync protocol 4

- Status: accepted 2026-10-02 (the owner chose A in `tasks/partition-converge/notes.md`)
- Date: 2026-10-02
- Deciders: project owner

## Context
A reopen (and an undo of a `do`) turns `x line pri:B` back into `(B) line`. It sends
`Completed = false`, the priority, and an `EditText` that deletes ` pri:B` at char offsets. Under
ADR 0034 a late edit is slotted in by stamp and the text replayed. When another device prepended to
the done line while apart, and that prepend is older than the reopen, the replay puts it in front of
the reopen's splice, and the splice deletes the wrong chars on every device alike: converged, but
garbled (`(B) b line:B`). The op says where to cut, not what, so no device can tell.

Two options were weighed: A, a reopen names the tag it drops; B, every `EditText` delete carries
the text it removes. B covers more but is still half a fix (inserts drift the same way), needs a
rule for finding text that moved, and changes every text edit. A fully fixes the reopen case.

## Decision
We will add an op that removes a tag by its key.

- `OpKind::RemoveTag { task, key }`, appended (`txtodo-model`, variant 7): remove the first
  `key:value` word of the task's description and one space next to it (the one before it, else the
  one after), as core's `Edit::remove_tag`. No such word: nothing changes. A key that is empty or
  holds a space or a `:` is refused. `txtodo_model::remove_tag` is the one implementation.
- `reconcile::change_ops` sends it instead of an `EditText` when the new description is exactly the
  old one with its first `pri:` word removed. Reopen, undo of a `do` and an editor save that drops
  the tag all go through there. Any other description change stays an `EditText`.
- The description's history (ADR 0034) keeps it as a change with no offset, so a replay applies it
  to whatever text is there.
- The Loro mirror applies it to its description text the same way. (The todo.txt mirror is gone since ADR 0038.)
- `PROTOCOL_VERSION` goes from 3 to 4: an older peer cannot decode variant 7 and would drop the
  link. Mixed v3/v4 devices refuse each other, which the TUI, the desktop and doctor already show
  (ADR 0035).
- Old logs keep their `EditText` reopens and replay as before.

## Consequences
- Good: a reopen drops the tag it means on every device, whatever was slotted in front of it.
- Bad: a v3 and a v4 device do not sync until both are upgraded.
- Bad: one more op kind for every exhaustive match (store, mirror, tree cache, history, sync).
- Not fixed: other concurrent text edits still drift by offset (ADR 0034's T1). That needs edits
  anchored to the text around them, a separate decision.
