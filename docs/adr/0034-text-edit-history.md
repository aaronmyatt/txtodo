# 0034 — Order concurrent text edits by stamp, from a kept history

- Status: accepted 2026-10-01 (the owner chose T1 in `tasks/partition-converge/notes.md`)
- Date: 2026-10-01
- Deciders: project owner

## Context
`EditText` (a task's description) and `NotesEdit` (a whole `notes.md`) are splices at char
offsets on the text their author saw. Two devices that edit one text at once each apply the
other's splice in arrival order and end with two texts (`line 1 a b` and `line 1 b a`). After the
placement fix (ADR 0033) this was the lab's main failure, mostly in `notes.md`. A HLC cannot tell
an edit made after seeing another from one made at the same time, so skipping the older one is
not enough.

## Decision
We will keep, per text, the text it started from and every edit since, in stamp order
(`text_history.rs`):

- The newest edit applies on the current text, as before.
- An edit older than one already applied is slotted in by stamp, and the text is rebuilt from the
  base with every kept edit in order; one that no longer fits is skipped. Every device holds the
  same base and edits, so every device rebuilds the same text.
- `DocState` keeps one history per task (64 edits), dropped when the description changes another
  way (a `SetField` that moves a priority into `pri:`) or the task leaves the document.
  `NotesState` keeps one per `notes.md` (256 edits). Past the bound the oldest edits fold into the
  base.
- Both are rebuilt at open from the log replay (`adopt_stamps`, `adopt_history`).
- After a notes import, the Loro mirror (which took the splices where they fell) gets one unlogged
  edit to the state's text: the mirror never decides bytes.

No wire change.

## Consequences
- Good: concurrent edits to one description or one `notes.md` end with one text everywhere.
- Bad: a replayed splice always applies, but on a text its author never saw it can land a little
  off from where they meant (an insert at offset 6 is at offset 6 of the rebuilt text).
- Bad: two devices whose histories began at different texts (a description changed by a
  completing `SetField` between two concurrent edits, or edits older than the bound) can still
  rebuild differently.
- Neutral: memory per edited task and notes file, up to the bounds.

## Alternatives considered
- T2. Each edit names the text it was made on: exact, but a wire change and a protocol bump.
- T3. Let the Loro mirror decide description and notes bytes: it merges by character already,
  but it breaks "the mirror never decides bytes".

## Amendment 2026-10-02
The first "Bad" above (a completing `SetField` between two concurrent edits) is narrowed. When
completing a line moves its priority into `pri:`, the description's history is no longer dropped:
the rewrite is kept as an append at its op's stamp (`Change::Append`), applied at the end of
whatever text the replay builds. A text edit that arrives late is then slotted in by stamp on every
device. Found by ADR 0035's digest check in the lab (lan-converge seed 202: two appends made
apart, one device completing the prioritized line, the appends in different orders). A
description changed any other way by a `SetField` still drops its history.

## Amendment 2026-10-02 (second)
The kept change is now `Change::Pri(letter)`: the first `pri:` tag set to the letter, else
` pri:X` added at the end, as core's `Edit::set_tag`. A priority changed on a done line (`pri:B`
swapped to `pri:C` in place) used to drop the history too; it now sets the letter of every
`Pri` change the history holds and of the base's tag (`TextHistory::swap_pri`), with no stamped
entry of its own. The priority is last writer wins, so only the newest value reaches it, and the
completion's tag shows that value in any arrival order. A stamped swap entry was tried first: it
put `pri:C` back on a line an older reopen had opened, and lost to an older priority that arrived
after a newer completion (lab sleep run 20261002-081812). When the rebuilt text still holds
another letter (a tag a text edit added: a `do` before 4b11aeb6 sent one), the history is dropped
as before. Every saved lab log replays to the same bytes as before.

Known gap: a reopen's text edit drops ` pri:B` by offset. An older edit slotted in front of it
(another device prepending to the done line) moves the text, and the splice cuts the wrong chars
on every device alike: converged, but garbled. The op says where to cut, not what, so no device
can tell; that needs a wire change (task partition-converge, `@human`).
