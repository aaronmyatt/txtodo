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
