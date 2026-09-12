# txtodo-store

## Purpose
SQLite op log, snapshots, projection cache at `<workspace>/.txtodo/oplog.db`. Plan M3, ADR 0004
(rusqlite bundled, WAL). As built 2026-09-11.

## Public interface
- `Store::open(path)` — creates, switches to WAL, applies `migrations/000N.sql` in order by
  `user_version` (now 2), refuses a newer schema. `user_version()`, `journal_mode()` for doctor and tests.
- Op log: `append(&[Op]) -> SeqRange` (one transaction, `MAX_APPEND_BATCH`),
  `for_file(file, since: Seq)`, `between(file, &Hlc, &Hlc)` (inclusive, HLC order),
  `last_seq()`. Reads return `Stored { seq, op }`, at most `MAX_OPS_PER_READ`.
- Sync heads (M4): `heads() -> BTreeMap<DeviceId, u64>` (≤ `MAX_DEVICES_PER_HEADS`),
  `head_of(device)`, `next_origin_seq(device)`, `ops_for(device, first, last)` — origin_seq is
  the op's rank in its device's HLC order, derived from rows, never a column.
- Projections: `put_projection(&Projection)` / `get_projection(file)`; `MAX_PROJECTION_BYTES`.
- Snapshots: `put_snapshot(file, &Snapshot { seq, state })` / `latest_snapshot(file)`.
- Meta: `meta_set(key, bytes)` / `meta_get(key)` — device id, later encrypted keys.
- Rows: `payload` is postcard of the whole `Op`; `principal`, `kind`, `file`, `hlc_*` are filters.

## Invariants
- Append-only op log: no `UPDATE`/`DELETE` statement exists in this crate (tests/oplog.rs greps).
  Projections, snapshots and meta are upserts and carry no history.
- Everything here is rebuildable from the files; deleting `.txtodo/` is the reset.
- Every read has an upper bound; every error names the operation and, when known, the path.
- Ids are stored as 16-byte big-endian BLOBs; wall times as i64 milliseconds.
- A device's ops are dense by construction (own ops always land; sync commits contiguous runs
  from `head + 1`), which is what lets `COUNT(*)` per device be the head.
- May depend only on: txtodo-model.
