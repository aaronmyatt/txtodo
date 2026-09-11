# txtodo-model

## Purpose
Task ids, HLC, Op/OpKind/Principal — the op model every later layer records, ships and merges.
Plan M3 (as built 2026-09-11). Workspace tree + progress (plan §3.2.5) is M5 and not here yet.

## Public interface
- Ids: `DeviceId`, `TaskId`, `OpId`, `TokenId` (ULID bits, `ulid()` for the core view);
  `FilePath::new` validates a workspace-relative `/`-separated path once, at the boundary.
- `Hlc { wall_ms, counter, device }`, `Hlc::zero`, `Hlc::tick(now_ms) -> Result<Hlc, HlcError>`.
- `Op { id, hlc, principal, file, kind }`; `OpKind` = Insert · SetField · EditText · Move ·
  NotesEdit · BlankInsert · BlankRemove; `set_field(task, field, value)` is the only SetField
  constructor; `FieldValue::{date, priority, quirks, as_date, as_quirks}` bridge core types.
- `TextEdit` mirrors `txtodo_core::TextEdit` with `From` both ways; `Principal` displays as
  `you@dev` / `agent:name@dev` / `external@dev`.
- Codec: `serde` derives everywhere; `postcard` is the payload format (store BLOB, M4 wire).

## Invariants
- Op model is the one the CRDT will use; do not fork it later.
- No I/O and no clock in this crate: `tick` takes `now_ms`; entropy for ids is the caller's.
- `Hlc::tick` is strictly monotone and never changes the device; overflow is `Err`, not a wrap.
- The `FieldValue::Quirks(u16)` bit order is `op.rs::ALL_QUIRKS`; append only.
- May depend only on: txtodo-core.
