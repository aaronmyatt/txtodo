# txtodo-store

## Purpose
SQLite op log, snapshots, projection cache at `<workspace>/.txtodo/oplog.db`. Plan M3, ADR 0004
(rusqlite bundled, WAL). As built 2026-09-12 (capability tokens, plan M6 data layer). Sidecar
identity fingerprints (docs/questions.md Q2, now the default identity mode) added 2026-09-13.
Known-devices table (plan M4 tasks/sync-device-remove) added 2026-09-13.

## Public interface
- `Store::open(path)` — creates, switches to WAL, applies `migrations/000N.sql` in order by
  `user_version` (now 6), refuses a newer schema. `user_version()`, `journal_mode()` for doctor and tests.
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
- Sidecar identity fingerprints (docs/questions.md Q2): `upsert_fingerprint(file, task,
  &Fingerprint, updated_at_ms)` (upsert, revives a tombstoned row), `retire_fingerprint(file,
  task, at_ms)` (idempotent, tombstones — never deletes — the row), `live_fingerprints(file)` /
  `tombstoned_fingerprints(file)` (≤ `MAX_FINGERPRINTS_PER_READ` each). What
  `crates/txtodo-daemon/src/identity/assign.rs` matches an external edit's fresh fingerprints
  against.
- Known devices (plan M4 tasks/sync-device-remove): `register_device(&NewDevice)` (upsert, revives
  a removed row — same idiom as fingerprints), `list_devices()` (≤ `MAX_DEVICES_PER_READ`, oldest
  paired first, includes removed rows), `device(id)` (single lookup), `remove_device(id, at_ms)`
  (idempotent tombstone upsert, `false` for an unknown device), `set_device_key_epoch(id, epoch)`
  (updates only `key_epoch`, `false` for an unknown device). `static_public` is a plain 32-byte
  array (this crate cannot depend on txtodo-sync's `DeviceStaticPublic`); the daemon converts at
  its own boundary. `last_known_wall_ms` is `None` until a real peer clock sample is observed —
  `txtodo doctor` reports "no clock sample yet" rather than guess one.

## Invariants
- Append-only op log: no `UPDATE`/`DELETE` statement exists in this crate (tests/oplog.rs greps).
  Projections, snapshots, meta, tokens, fingerprints and devices are upserts and carry no history
  of their own; a removed device row is kept, never deleted.
- Everything here is rebuildable from the files **except sidecar fingerprints**: a tagged
  workspace can always re-derive ids by re-reading `id:` tags, but a sidecar workspace's identity
  lives only here — deleting `.txtodo/` loses task-identity continuity for it (known tradeoff,
  plan `floofy-swinging-brooks.md`).
- Every read has an upper bound; every error names the operation and, when known, the path.
- Ids are stored as 16-byte big-endian BLOBs; wall times as i64 milliseconds.
- A device's ops are dense by construction (own ops always land; sync commits contiguous runs
  from `head + 1`), which is what lets `COUNT(*)` per device be the head.
- May depend only on: txtodo-model.
