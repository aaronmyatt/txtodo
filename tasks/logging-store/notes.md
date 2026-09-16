# txtodo-store: instrument open, commit, heads and projection/snapshot writes (root todo.txt line 36)

## Goal

`txtodo-store` (4,147 lines) has zero `tracing` calls — no visibility into WAL/migration
behaviour on open, the durability transaction every commit funnels through, the per-device head
ledger the sync protocol reads, or the projection/snapshot writes the daemon's mirror and replay
depend on. This task adds `tracing = "0.1"` (event/span emission only — this low-level crate never
wires a subscriber, that is `txtodo-daemon`'s job, already done) and instruments exactly the four
sites root todo 36 names: `lib.rs:58` `open`, `commit.rs`'s one `commit_change_with` transaction
(the durability funnel `actor.rs:336`'s `persist_change` calls into), `heads.rs:36/61/73`
(`heads`/`head_of`/`next_origin_seq`), and `projections.rs`'s `put_projection`/`put_snapshot`.
Never changes observable behaviour — every new code path is a `tracing` call and nothing else.

## Design

### Severity convention (same as `logging-daemon-swallowed-errors`/`logging-sync-crate`)

Routine/expected outcomes stay `debug` (every site here is routine — a schema-too-new open is the
only real failure named). A schema-too-new open (an old build touching a file written by a newer
one) is `warn`: a real, actionable operational condition, not a crash-safety assertion.

### Sites

1. **`lib.rs::open`** (line 58): thin `#[instrument(skip_all, fields(path = %path.display()))]`
   wrapper around a renamed `open_inner` (matches `actor.rs`/`session.rs`/`reconcile.rs`'s own
   wrapper+inner idiom — `#[instrument]` itself costs `cognitive_complexity` points, cheapest fix
   is splitting before it's ever a problem here). `open_inner` now also counts migrations actually
   applied (previously discarded by the `for` loop) and returns `(Store, migrations_applied,
   found_version)`. The `found > SCHEMA_VERSION` early return becomes `return
   Err(log_schema_too_new(found, SCHEMA_VERSION))` — a one-macro-call leaf function (`warn`) that
   both logs and builds the error, so the `if` branch itself never contains a bare macro call
   (`logging-sync-crate`'s hard-learned rule: a macro call directly inside a branch costs points on
   its own, independent of `#[instrument]`). `open`'s own body, after `open_inner` returns,
   confirms the actual `journal_mode` pragma (not assumed) and logs one `store_opened` event
   (`journal_mode`, `schema_from`, `schema_to`, `migrations_applied`) via a leaf function — no
   branch in `open` itself, so no complexity cost from the call site.
2. **`commit.rs::commit_change_with`**: same wrapper+inner split, renamed body to
   `commit_change_with_inner`. `#[instrument(skip_all, fields(file = %projection.file, ops =
   ops.len(), bytes = projection.bytes.len(), hash = %hex8(&projection.hash)))]` on the wrapper —
   span fields carry the transaction's shape (ops count, projection size, hash) for the whole
   durability boundary, including whatever `upsert_projection` itself logs underneath (item 4).
   The wrapper logs one `commit_landed` event (`seq` = the appended range's last seq, or none)
   via a leaf function after `commit_change_with_inner` returns — the outcome, not a repeat of the
   span's own fields. `commit_change` (the no-extras convenience wrapper) is untouched; it already
   just calls `commit_change_with`, which now carries the instrumentation for both callers.
3. **`heads.rs`**: `heads()` (line 36), `head_of(device)` (61), `next_origin_seq(device)` (73) each
   get `#[instrument(skip_all, fields(..))]` — no wrapper+inner split needed, none of the three has
   a branch nesting its return, so a leaf log function called unconditionally right before the
   final `Ok(..)` costs nothing extra. `heads()` logs `device_count`; `head_of`/`next_origin_seq`
   log `device` (already a `Display` id, no payload) and the count/seq itself. No
   duplicate/dropped-op event: `heads.rs` is a pure read ledger (`COUNT(*)` derived, no `INSERT`
   anywhere in this file) — the sync protocol's own accept/reject-as-duplicate decision lives in
   `txtodo-daemon`, not here, so there is no real code path in this file to log a drop from. Noted
   as a deliberate no-op rather than fabricating an event with nothing behind it.
4. **`projections.rs`**: the shared `upsert_projection(conn, p)` free function (used by both
   `Store::put_projection` and `commit.rs::commit_change_with_inner` — instrumenting here covers
   both real write paths from one place, no duplicated event) logs one `projection_written` event
   (`file`, `bytes`, `hash`) right before returning `Ok`, and the existing `if p.bytes.len() >
   MAX_PROJECTION_BYTES` branch becomes `return Err(log_projection_too_large(file, len))` (`warn`,
   leaf function, same shape as `lib.rs`'s `log_schema_too_new`). `Store::put_snapshot` gets its
   own `#[instrument(skip_all, fields(file = %file, seq = snap.seq.0))]` plus a `snapshot_written`
   leaf event (`bytes` = `snap.state.len()`) — `put_projection`/`put_snapshot` both get
   `#[instrument]` wrappers even though the real event lives in the shared free function for
   projections, so a span shows up around either write regardless of call path.

### `hex8`

A private `pub(crate) fn hex8(hash: &[u8; 32]) -> String` (first 8 hex chars, same idiom as
`txtodo-daemon::expected::hex8`, re-derived rather than reached across the crate boundary — this
crate may depend only on `txtodo-model`) lands in `lib.rs` next to `SCHEMA_VERSION`, used by
`commit.rs` and `projections.rs`.

## Placement

Every leaf log function contains exactly one `tracing` macro call, matching this repo's
established rule. No function touched here starts near the `cognitive_complexity` budget (10); the
wrapper+inner split is used only for `open`/`commit_change_with` since those are the two sites with
real branching (`open`'s migration loop + schema check, `commit_change_with`'s extras landing).

## Edge cases

- `open`'s `debug_assert_eq!(MIGRATIONS.last()..., Some(SCHEMA_VERSION))` and
  `debug_assert_eq!(store.user_version()?, SCHEMA_VERSION)` stay exactly as they are — assertions,
  not logging, out of scope for this pass.
- No log event for `heads.rs`'s "dropped/duplicate op" case — see site 3 above, not a real code
  path in this file.
- Nothing here logs projection *bytes* or *file line text* — only the file path, byte count and
  blake3 hash, matching this crate's own `StoreError`/`prev_hash` precedent of never storing or
  logging plaintext task content.

## Acceptance

- Every site named in the backlog line (`lib.rs:58`, `commit.rs`'s `commit_change_with`,
  `heads.rs:36/61/73`, `projections.rs`'s projection/snapshot writes) emits a span and/or event, no
  payload/line text/secrets in any field — ids, counts, hashes, kinds and named event labels only.
- `cargo fmt -p txtodo-store -- --check`, `cargo clippy -p txtodo-store --all-targets -- -D
  warnings`, `cargo test -p txtodo-store` green after every commit; no `#[allow]`/`#[expect]`
  anywhere.
- Existing test suite (this crate installs no in-process subscriber, same stance
  `logging-sync-crate`/`logging-daemon-swallowed-errors` took) proves the instrumentation compiles
  in without changing behaviour; `actor.rs`'s own call into `persist_change` →
  `commit_change_with` needs no change (same signature).

## As built (2026-09-16, agent)

Built exactly to the design above, four commits, one per subtask line:

1. `1b528f3` — `Cargo.toml` (`tracing = "0.1"`), `lib.rs`: `hex8` helper, `open`/`open_inner`
   wrapper split, `log_schema_too_new` (warn, leaf, builds+logs the error), `log_store_opened`
   (debug: `journal_mode`, `schema_from`, `schema_to`, `migrations_applied` — the loop now counts
   what it applies instead of discarding it).
2. `cc9c9f6` — `commit.rs`: `commit_change_with`/`commit_change_with_inner` wrapper split, span
   fields `file`/`ops`/`bytes`/`hash` on the wrapper, `log_commit_landed` (debug: the appended seq
   range) after the transaction returns. `commit_change` (the no-extras convenience call) needed no
   change — it already just calls `commit_change_with`. `Cargo.lock` (the `tracing` dependency
   addition) landed in this commit rather than the first, since it was still unstaged when commit 1
   closed.
3. `fe3fd50` — `heads.rs`: `heads()`/`head_of()`/`next_origin_seq()` each get
   `#[instrument(skip_all, ...)]` plus one leaf debug event (`log_heads_read`/`log_head_of`/
   `log_next_origin_seq`) called unconditionally before their final `Ok`. No wrapper+inner split
   needed — none of the three nests its return inside a branch.
4. `686e41b` — `projections.rs`: the shared `upsert_projection` free function (used by both
   `Store::put_projection` and `commit.rs`'s transaction) logs `projection_written` (debug:
   `file`/`bytes`/`hash`) and its size-limit rejection became `log_projection_too_large` (warn,
   leaf, same error-building shape as `log_schema_too_new`). `put_snapshot` got its own span
   (`file`/`seq`) plus `log_snapshot_written` (debug: `bytes`).

### Deviations from the plan

- **No duplicate/dropped-op event in `heads.rs`.** The design anticipated this might not be a real
  code path and confirmed it on inspection: `heads.rs` has no `INSERT`/write of any kind — every
  function is a `COUNT(*)`/`SELECT` derived from the `ops` table (indexed by `ops_device_hlc`). The
  sync protocol's own accept/reject-a-duplicate-run decision lives in `txtodo-daemon`
  (`lan_session_dispatch.rs` et al., already covered by `logging-daemon-swallowed-errors`), not
  here. Logging a "dropped op" event with nothing behind it would have been fabricated telemetry —
  left out rather than invented, as the brief's own "if that's a real code path" hedge anticipated.
- **`commit_change_with`'s span carries `bytes`/`hash` as span fields, not a duplicate log event.**
  The shared `upsert_projection` free function (site 4) already emits `projection_written` with the
  same `bytes`/`hash` at the actual write site, used by both call paths (`put_projection` and the
  commit transaction). Repeating an identical event at the `commit_change_with` layer would have
  been pure noise; putting the values on the span instead means every event emitted during the
  transaction (including the nested `projection_written`) is still correlatable by span context
  without a second copy of the same numbers.
- **`Cargo.lock` landed in commit 2, not commit 1.** A same-session ordering detail: it was still
  unstaged when commit 1 (`Cargo.toml` + `lib.rs`) closed, so it rode along with commit 2 instead.
  No functional effect — both commits are independently green.

### Verification

- `cargo fmt -p txtodo-store -- --check`: clean, every commit.
- `cargo clippy -p txtodo-store --all-targets -- -D warnings`: clean on a `cargo clean -p
  txtodo-store` rebuild after every commit — no `#[allow]`/`#[expect]` anywhere. Note: the
  PostToolUse lint hook reported transient `cognitive_complexity` hits (`head_of`,
  `next_origin_seq`, `put_projection`, `put_snapshot` all at 11/10) mid-edit, before the matching
  `log_*` function existed yet or before a stale incremental artifact was cleared; every one of
  those functions passed clean on a fully clean rebuild once its edit was complete, and stayed
  clean through the final full-crate reverification below. No wrapper+inner split was needed for
  `heads.rs`/`projections.rs` — only `open`/`commit_change_with` (the two sites with real
  pre-existing branching) needed one.
- `cargo test -p txtodo-store`: 55 tests across 11 files, all green, every commit and on final
  reverification.
- `.claude/scripts/check-boundaries.sh`: clean — `tracing` is an external crate, never touched by
  the `txtodo-*` edge check.
- `cargo check -p txtodo-daemon`: clean after every commit — `commit_change_with`'s signature is
  unchanged, `actor.rs::persist_change`'s call site needed no edit.
- File-length budget: every touched file landed well under 400 (`lib.rs` 137, `commit.rs` 235,
  `heads.rs` 130, `projections.rs` 172) — no prose trimming needed, unlike the daemon/sync passes.
- No in-process subscriber assertion on captured JSON lines: same stance `logging-sync-crate`/
  `logging-daemon-swallowed-errors` took — this crate deliberately never wires
  `txtodo_telemetry::init`, only the daemon binary does, so proof here is the existing test suite
  staying green with the instrumentation compiled in, not a captured-output test.

### Deliberately out of scope

- `ops.rs` (not named by the backlog line — `heads.rs` reads `ops` but never writes it),
  `flags.rs`'s `upsert_mirror_on` (the Loro mirror, a different write from the snapshot/projection
  pair actually named), `devices.rs`/`registry.rs`/`tokens.rs`/`identity.rs`/`identity_store.rs`
  (none named), and `get_projection`/`latest_snapshot`/`meta_get` (reads, not the "writes" the
  backlog line asks for).
- Any other `+m11 @observability` backlog line, any `txtodo-daemon`/`txtodo-sync` file (both
  already instrumented by prior tasks), or any file outside `crates/txtodo-store/`,
  `tasks/logging-store/` and this one root todo.txt line.
