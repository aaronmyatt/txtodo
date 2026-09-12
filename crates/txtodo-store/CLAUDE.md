# txtodo-store

## Purpose
SQLite op log, snapshots, projection cache at `<workspace>/.txtodo/oplog.db`. Plan M3, ADR 0004
(rusqlite bundled, WAL). As built 2026-09-12 (capability tokens, plan M6 data layer).

## Public interface
- `Store::open(path)` — creates, switches to WAL, applies `migrations/000N.sql` in order by
  `user_version` (now 4), refuses a newer schema. `user_version()`, `journal_mode()` for doctor and tests.
- Op log: `append(&[Op]) -> SeqRange` (one transaction, `MAX_APPEND_BATCH`),
  `for_file(file, since: Seq)`, `between(file, &Hlc, &Hlc)` (inclusive, HLC order),
  `last_seq()`. Reads return `Stored { seq, op }`, at most `MAX_OPS_PER_READ`.
- Sync heads (M4): `heads() -> BTreeMap<DeviceId, u64>` (≤ `MAX_DEVICES_PER_HEADS`),
  `head_of(device)`, `next_origin_seq(device)`, `ops_for(device, first, last)` — origin_seq is
  the op's rank in its device's HLC order, derived from rows, never a column.
- Review flags (M4): `raise_flag(&ReviewRow)`, `open_flags(file)` (≤ `MAX_OPEN_FLAGS_PER_READ`,
  oldest first), `clear_flag(file, task, at_ms)` (idempotent upsert). Mirror: `put_mirror(file,
  snapshot, seq)` / `get_mirror(file)` — the Loro snapshot as of a log position.
- Projections: `put_projection(&Projection)` / `get_projection(file)`; `MAX_PROJECTION_BYTES`.
- Snapshots: `put_snapshot(file, &Snapshot { seq, state })` / `latest_snapshot(file)`.
- Meta: `meta_set(key, bytes)` / `meta_get(key)` — device id, later encrypted keys.
- Rows: `payload` is postcard of the whole `Op`; `principal`, `kind`, `file`, `hlc_*` are filters.
- Capability tokens (M6, design §6.2): `create_token(&NewToken)` (only `blake3(secret)` lands,
  never the plaintext), `list_tokens()` (≤ `MAX_TOKENS_PER_READ`, includes revoked/expired rows —
  the wire boundary decides what a client sees), `revoke_token(id, at_ms)` (upsert, idempotent),
  `verify_token(secret, now_ms) -> Result<TokenId, TokenError>` — the enforcement primitive a
  future request-time agent-auth path calls; nothing in this repo calls it over the wire yet.

## Invariants
- Append-only op log: no `UPDATE`/`DELETE` statement exists in this crate (tests/oplog.rs greps).
  Projections, snapshots, meta and tokens are upserts and carry no history of their own.
- Everything here is rebuildable from the files; deleting `.txtodo/` is the reset.
- Every read has an upper bound; every error names the operation and, when known, the path.
- Ids are stored as 16-byte big-endian BLOBs; wall times as i64 milliseconds.
- A device's ops are dense by construction (own ops always land; sync commits contiguous runs
  from `head + 1`), which is what lets `COUNT(*)` per device be the head.
- May depend only on: txtodo-model.
