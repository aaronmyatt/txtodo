# txtodo-model

## Purpose
Task ids, HLC, Op/OpKind/Principal — the op model every later layer records, ships and merges.
Plan M3 (as built 2026-09-11). Workspace tree + progress (plan §3.2.5) is M5 and not here yet.
Identity mode (tagged vs. sidecar, `docs/questions.md` Q2) added 2026-09-13.

## Public interface
- Ids: `DeviceId`, `TaskId`, `OpId`, `TokenId` (ULID bits, `ulid()` for the core view); `Ulid` re-exported;
  `FilePath::new` validates a workspace-relative `/`-separated path once, at the boundary.
- `IdentityMode::{Tagged, Sidecar}` — which way a workspace establishes task identity, minted once
  and fixed for its lifetime (`txtodo-daemon`'s `workspace.rs`). `Fingerprint` — the pure-data shape
  sidecar mode matches on (creation date, projects, contexts, normalised description, line index);
  no matching algorithm lives here (it needs a string-distance dependency this crate must not
  carry). `CostWeights::DEFAULT` — the v1 cost-function weights and match threshold from Q2;
  `crates/txtodo-daemon/src/identity/` is where they're actually applied.
- `Hlc { wall_ms, counter, device }`, `Hlc::zero`, `Hlc::tick(now_ms)` (send rule),
  `Hlc::merge(remote, now_ms)` (receive rule, Kulkarni §3), both `-> Result<Hlc, HlcError>`;
  `HlcError::{Overflow, PeerAhead}`; `Skew::check(peer_ms, local_ms) -> Skew::{Ok, Behind, Ahead}`
  against `MAX_PEER_SKEW_AHEAD_MS` / `MAX_PEER_SKEW_BEHIND_MS` (5 min each).
- `Op { id, hlc, principal, file, kind }`; `OpKind` = Insert · SetField · EditText · Move ·
  NotesEdit · BlankInsert · BlankRemove; `set_field(task, field, value)` is the only SetField
  constructor; `FieldValue::{date, priority, quirks, as_date, as_quirks}` bridge core types.
- `Op::signing_bytes()` — the canonical postcard bytes a device signature covers. Signature lives in
  the store column, not in `Op`, so there is nothing to strip. Golden in `goldens/op_signing.postcard`.
- `TextEdit` mirrors `txtodo_core::TextEdit` with `From` both ways; `Principal` displays as
  `you@dev` / `agent:name@dev` / `external@dev`.
- Codec: `serde` derives everywhere; `postcard` is the payload format (store BLOB, M4 wire).

## Invariants
- Op model is the one the CRDT will use; do not fork it later.
- No I/O and no clock in this crate: `tick` takes `now_ms`; entropy for ids is the caller's.
- `Hlc::tick` is strictly monotone and never changes the device; overflow is `Err`, not a wrap.
- `Hlc::merge` returns a stamp greater than both inputs; a peer more than 5 min ahead is
  refused (`PeerAhead`) and the clock is untouched on any `Err`. Behind is safe; callers warn.
- The `FieldValue::Quirks(u16)` bit order is `op.rs::ALL_QUIRKS`; append only.
- `OpKind` is append-only by **security**, not style: a variant insert/reorder changes every signed
  op's `signing_bytes`. No `HashMap`/`HashSet` anywhere reachable from `Op` — `BTreeMap` or sorted
  `Vec` only.
- May depend only on: txtodo-core.
