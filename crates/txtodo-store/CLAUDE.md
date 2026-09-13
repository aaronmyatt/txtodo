# txtodo-store

## Purpose
SQLite op log, snapshots, projection cache at `<workspace>/.txtodo/oplog.db`. Plan M3, ADR 0004
(rusqlite bundled, WAL). As built 2026-09-12 (capability tokens, plan M6 data layer). Sidecar
identity fingerprints (docs/questions.md Q2, now the default identity mode) added 2026-09-13.
Known-devices table (plan M4 tasks/sync-device-remove) added 2026-09-13. The device-global
workspace registry's raw rows (ADR 0025, task `daemon-workspace-registry`) added the same day —
its own separate database file, not `oplog.db`; see `registry.rs`'s own doc and Public interface
entry below.

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
- Workspace registry (ADR 0025, task `daemon-workspace-registry`): `registry.rs`'s `Registry` — a
  **separate** SQLite database from `Store`'s own `oplog.db` (a different file, its own
  `PRAGMA user_version` sequence starting at 1, embedded from `registry_migrations/0001.sql` next
  to this crate's existing `migrations/`), because it is device-global, not per-workspace, and has
  no natural single `<workspace>/.txtodo/` to live under — `txtodo-daemon`'s
  `workspace_registry.rs` resolves where its file actually goes on disk (device-global, not this
  crate's concern) and owns id minting; this crate only stores the rows. `Registry::open(path)`,
  `insert(&NewWorkspaceEntry)` (a unique partial index on `(root) WHERE removed_at IS NULL` is the
  actual backstop against two active rows for the same root — the caller is expected to check
  `find_active_by_root` first), `find_active_by_root(root)`, `get(id)`, `list_active()` (≤
  `MAX_WORKSPACES_PER_READ`, oldest-registered first), `remove(id, at_ms)` (idempotent tombstone,
  the same upsert idiom as `remove_device`; `false` only for a wholly unknown id). `WorkspaceId` is
  a ULID newtype, minted (never derived from the path) the same way `DeviceId` is — kept in this
  crate rather than promoted to `txtodo-model` for now, since only this crate and `txtodo-daemon`
  need it (`tasks/daemon-workspace-registry/notes.md` has the reasoning). `root` is stored as the
  canonicalized absolute path (a plain TEXT column) purely to detect "already registered"; it is
  never read back as identity, and this table never opens, reads or even constructs a path under
  it — the workspace's own `.txtodo/oplog.db` is untouched by every operation here, by construction
  (this module has no code path that names it).

## Invariants
- Append-only op log: no `UPDATE`/`DELETE` statement exists in this crate (tests/oplog.rs greps).
  Projections, snapshots, meta, tokens, fingerprints and devices are upserts and carry no history
  of their own; a removed device row is kept, never deleted. `registry.rs` is exempt from
  `tests/oplog.rs`'s grep (scoped to `lib.rs`/`ops.rs`/`error.rs` only, same as `devices.rs`) and
  uses a real `UPDATE`-shaped upsert (`INSERT ... ON CONFLICT(id) DO UPDATE SET removed_at =
  ...`) for the same tombstone reason `devices.rs` does — never a bare `UPDATE`/`DELETE`.
- Everything here is rebuildable from the files **except sidecar fingerprints**: a tagged
  workspace can always re-derive ids by re-reading `id:` tags, but a sidecar workspace's identity
  lives only here — deleting `.txtodo/` loses task-identity continuity for it (known tradeoff,
  plan `floofy-swinging-brooks.md`). The workspace registry is different again: losing it loses no
  task data at all (every workspace's own `.txtodo/oplog.db` is untouched either way), only the
  device's memory of which directories to manage — re-registering a still-intact directory finds
  its op log exactly where it was.
- Every read has an upper bound; every error names the operation and, when known, the path.
- Ids are stored as 16-byte big-endian BLOBs; wall times as i64 milliseconds.
- A device's ops are dense by construction (own ops always land; sync commits contiguous runs
  from `head + 1`), which is what lets `COUNT(*)` per device be the head.
- May depend only on: txtodo-model.
