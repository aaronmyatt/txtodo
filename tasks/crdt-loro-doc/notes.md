# Loro document per file with the design §4.2 shape, OpKind ⇄ Loro both ways (plan M4)

The op log stays authoritative for *persistence and transport* (`txtodo-store` holds our `Op`);
Loro is the *merge engine*. So this task is a pure translation layer plus a document shape — no
behaviour change on one device. Loro Rust API: <https://docs.rs/loro> · concepts:
<https://loro.dev/docs/tutorial/get_started>.

## Document shape (design §4.2)

| design node | Loro container | API |
|---|---|---|
| `files: Map<FilePath, MovableList<TaskId>>` | one `LoroMovableList` per file, keyed `files/<path>` | <https://loro.dev/docs/tutorial/list> |
| `tasks: Map<TaskId, Task>` | `LoroMap` `tasks`, each value a nested `LoroMap` | <https://loro.dev/docs/tutorial/map> |
| `description` | `LoroText` inside the task map | <https://loro.dev/docs/tutorial/text> |
| every other task field | LWW register = a plain key in the task `LoroMap` | — |

Loro's movable list has a native `mov(from, to)`, which is why ADR 0006 picked it over Automerge
(there a move is delete+insert and loses identity). Blank lines are entries in the M3 `DocState`
(`state.rs` `Entry::Blank`); they are **not** tasks, so they need their own list entries — see the
open question below.

## The one real design decision: whose LWW wins

Design §4.2 says LWW registers resolve by **our** HLC (wall + counter + device id). Loro resolves
map-key conflicts by **its own** Lamport clock + peer id, which is not our HLC and is not something
we can substitute. Two ways out:

- **A — store the stamp in the value.** Each LWW key holds `{ value, hlc }` and we write only if the
  incoming `hlc` beats the stored one. Loro's own LWW becomes a tiebreak we never rely on.
- **B — let Loro's ordering be the truth** and drop HLC to a display/ordering aid.

Take **A**. It keeps design §4.2 honest, keeps the clock-skew guard (todo `+m4 @model`) meaningful,
and makes the merge rule readable in one function instead of hidden in a dependency. Cost: every
LWW read unwraps a struct. Write the ADR for it as part of this task.

## OpKind ⇄ Loro mapping

`OpKind` is closed (`crates/txtodo-model/src/op.rs`); both directions must be exhaustive matches.

| `OpKind` | to Loro | from Loro (event → op) |
|---|---|---|
| `Insert{task, after, line}` | `list.insert(idx_after(after)+1, task)` + populate task map | list insert event |
| `SetField{task, field, value}` | `task_map.insert(field_key, {value, hlc})` if hlc wins | map diff on that key |
| `EditText{task, edits}` | replay `TextEdit`s onto the `LoroText` | text delta |
| `Move{task, after, to_file}` | same file → `list.mov`; cross-file → delete + insert | list move / paired events |
| `NotesEdit` | M5 — a separate text doc, not in this doc | — |
| `BlankInsert`/`BlankRemove` | list insert/delete of a blank sentinel | list event |

`idx_after(None)` = 0. `Move` across files is the only op that touches two containers; it must be
one Loro transaction so a peer never observes the task in neither list.

## Boundaries and budgets

`txtodo-crdt` may depend on `txtodo-model`, `txtodo-store`, `txtodo-core` only
(`.claude/budgets.json` `allowedDeps`) — `loro` is a new dependency and needs human sign-off plus a
`deny.toml` pass before any code lands. Split as `doc.rs` (shape + open), `to_loro.rs`,
`from_loro.rs`, `lww.rs`; each fn ≤ 60 lines. Bound the event loop: an import applies
`ops.len() <= batch.len()` ops, asserted.

## Open questions for the human

- Blank lines: sentinel `TaskId`s in the list, or a parallel `LoroList<bool>`? Sentinels keep one
  ordered structure but pollute the `tasks` map. Leaning sentinel with a reserved ULID prefix.
- Do we keep Loro's own oplog on disk at all, or re-hydrate the doc from our `Op` log on start?
  Re-hydrating is simpler and makes our log the single source of truth; it costs startup time on a
  long history. Snapshot-on-close (`ExportMode::Snapshot`) is the middle road.
