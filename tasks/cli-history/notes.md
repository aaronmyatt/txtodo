# txtodo log, blame <line>, undo, checkout <iso-datetime> --stdout (plan M3)

Design §4.8: history is a view; the on-disk file is always "now". Undo is inverse ops that sync
(M4), so it is a daemon operation, not a CLI file rewrite.

## Timestamps
`checkout <iso-datetime>` parses with `jiff` (already a CLI dependency; https://docs.rs/jiff) in
the local zone (ADR 0011: local dates, no timezones in the file) and sends RFC 3339 over gRPC.
The daemon converts to an `Hlc { wall_ms, counter: u16::MAX, device: max }` upper bound so "at
09:00" includes every op stamped 09:00.

## Inverse ops (daemon side)
| op | inverse |
|---|---|
| `Insert` | `SetField Deleted=true` |
| `SetField f = v` | `SetField f = prev` (prev read from the state before apply) |
| `EditText e` | `EditText diff_text(after, before)` |
| `Move a→b` | `Move b→a` |
| `BlankInsert/Remove` | the other |
Inverse ops carry `Principal::User` (the person ran undo) and a `tag: undo_of = OpId` so `log`
can render "undo of …". Undoing an undo appends the original again: no separate redo stack.

## Checkout replay
`latest_snapshot(file)` at or before the Hlc, then `ops between(snapshot.hlc, at)` applied to a
scratch state, materialised. Bounded by `MAX_OPS_PER_READ`; longer histories replay in pages.
Snapshots every `SNAPSHOT_EVERY_OPS = 500` (design §4.4 "every N ops") are written by the actor.

## Output
Human table widths fixed; `--json` emits one object per op with `hlc`, `principal`, `kind`,
`task`, `summary`. Line text in summaries is truncated to `SUMMARY_MAX_CHARS = 60`.
