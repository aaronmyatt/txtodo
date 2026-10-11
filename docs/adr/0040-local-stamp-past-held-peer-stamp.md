# 0040 — A local edit is stamped past the newest peer stamp its document holds

- Status: accepted 2026-10-11 (decided by the agent; the owner asked for `@human` lines to be
  decided and the reasoning recorded, `tasks/lab-clock-skew/notes.md`)
- Date: 2026-10-11
- Deciders: agent, on the owner's standing instruction
- Amends: the skew-guard decision (`tasks/model-hlc-skew-guard`, option A)

## Context
The HLC refuses to merge a peer stamp more than 5 minutes ahead of this device's wall clock
(`Hlc::merge`, `PeerAhead`), so one bad clock cannot drag every device's clock forward. The peer's
op is still applied. A local edit typed after it then gets a stamp *older* than the line it was
typed on. Text and order are rebuilt in stamp order (ADR 0033, 0034), so the local edit is slotted
in front of the peer's, where it may not fit: lab clock-skew seed 777 lost a1's notes append on
both devices, and a todo.txt move snaps back.

## Decision
A document actor remembers the newest peer stamp it refused to merge (its *floor*). A local stamp
that is not already past the floor is moved just past it: the floor's wall and counter + 1, with
this device's id. The device clock is otherwise left alone, and only the document that holds the
far stamp is affected. A floor more than 24 hours ahead of this device's wall is treated as a
broken clock: logged (`stamp_floor_too_far`) and not followed.
Built in `crates/txtodo-daemon/src/stamp_floor.rs`, used by `FileActor::tick` and
`NotesActor::tick`; the floor is set where `sync_ops.rs` and `notes_clock.rs` log
`sync_stamp_not_merged`.

## Options
- A (chosen): stamp past the held floor, per document, up to 24 h ahead.
- B: merge far stamps anyway (drop the skew guard): every document and the device clock follow a
  bad clock, the problem the guard was made for.
- C: hold back far peer ops until our clock catches up: tried and reverted 2026-10-03 (7249bffe,
  f5b9fe99); a device whose own clock is behind stalls on every correct peer, and a device cannot
  tell whose clock is wrong.
- D: leave it (rare: needs clocks 5+ minutes apart and both editing one file): data loss, even
  rare, is the one thing a sync tool must not do quietly.

## Consequences
- Good: an edit always sorts after what the editor saw, the "local ops are newest" rule ADR 0033
  and 0034 lean on, inside the bound and past it.
- Bad: a document touched by a far-ahead peer carries stamps up to that peer's clock; history
  times for local edits there read in the peer's future. Bounded by 24 h.
- Neutral: the floor lives in memory; after a restart the actor's clock adopts the newest logged
  stamp anyway (`adopt_logged_clock`, `recover`), which covers the same case.
