# txtodo-crdt: instrument the merge-decision sites (root todo.txt line 36)

## Goal

`txtodo-crdt` (3,995 lines) has zero `tracing` calls — the six sites root todo 36 names are where
merge *outcomes* are decided (LWW arbitration, import/export, same-word conflict detection,
delete-vs-edit resurrection, op translation in both directions), not just data flow, and currently
emit nothing. Adds `tracing = "0.1"` (event/span emission only — this low-level crate never wires
a subscriber, that is `txtodo-daemon`'s job) and instruments exactly: `lww.rs:60 write_if_newer`
(ADR 0013 arbitration), `doc/sync.rs:72 import` and `:50 export_updates_since`, `review.rs:66
detect`, `resurrect.rs`'s `resolve` (delete-vs-edit, `specs/conflicts.md` rows 6-7), `to_loro.rs:54
apply`, `from_loro.rs:65 from_batch`. Never changes observable behaviour — every new code path is a
`tracing` call and nothing else.

## Design

### Severity convention (same as `logging-store`/`logging-sync-crate`)

Every site here is a routine, expected merge outcome (there is no "this failed" branch distinct
from the existing `Result` error paths, which callers already see) — every new event is `debug`.

### Sites

1. **`lww.rs::write_if_newer`** (line 60, ADR 0013's arbitration function): thin `#[instrument]`
   wrapper around a renamed `write_if_newer_inner` (`#[instrument]` on the real body risks the
   `cognitive_complexity` budget once the branch is there). Span fields: `key` only (a field name
   like `"desc"`/`"completed"`, never the value). `write_if_newer_inner` now also returns the
   register's *existing* stamp (`Option<Hlc>`) alongside the `bool` it already returned, so the
   wrapper can log the actual arbitration: `log_lww_write` (leaf, one `debug!` call) records
   `incoming_wall_ms`/`incoming_device` (the writer trying to land), `existing_wall_ms`/
   `existing_device` (`None` the first time a key is written), and `wrote` (`true` = incoming's HLC
   won and the register now holds it, `false` = the existing stamp was already newer or equal and
   nothing changed) — this is the actual "which side's HLC won" the backlog line asks for, not just
   a bool. Never logs `value`: an LWW register may hold a task description.
2. **`doc/sync.rs::import`** (line 72): wrapper/`import_inner` split (the function already has real
   branching — frontier computation, the resurrect hook). Span: `bytes = bytes.len()`. One
   `log_import_landed` leaf event after: `applied` (whether any new op landed at all — the
   coarsest, always-meaningful merge fact; the four frontiers themselves are `loro::Frontiers`,
   internal structure with no small scalar summary worth a field, and are already the typed
   `Imported` return value a caller can inspect directly).
   **`export_updates_since`** (line 50): no branching to split out — `#[instrument(skip_all,
   fields(since_bytes = since.len()))]` plus one `debug!` for `exported_bytes = bytes.len()` right
   before the existing `debug_assert!` and return.
3. **`review.rs::detect`** (line 66, same-word conflict flags): wrapper/`detect_inner` split (real
   branching: two early returns, a diff walk, per-file capping). Span: none needed beyond what the
   caller's own span already carries — `skip_all` with no extra fields, since `imported.applied` is
   about to be logged by `doc/sync.rs::import`'s own span/event one level up, and the task/file
   identities inside a flagged conflict are exactly the content this crate must never log. One
   `log_review_detected` leaf event: `flags = review.flags.len()`, `overflowed =
   review.overflowed.len()` — counts only, never a `ReviewFlag`'s `task`/`file`/`mine`/`theirs`.
4. **`resurrect.rs::resolve`** (delete-vs-edit/delete-vs-complete, `specs/conflicts.md` rows 6-7):
   wrapper/`resolve_inner` split. The existing `loses = delete_loses(mine, their) ||
   delete_loses(their, mine)` becomes two named bools (`mine_loses`/`their_loses`) so the *direction*
   of the resolved conflict is visible — this is the one site where "what actually got decided"
   needs a field of its own, not just a count: `log_task_resurrected` (leaf, one `debug!` call
   inside the existing `if` branch — a plain function call in a branch costs nothing, only a bare
   macro call would) fires once per task whose delete was overridden, with `task` (a `TaskId`,
   an identifier, never content), `mine_delete_lost`/`their_delete_lost` (which side's proposed
   delete was the one undone) and `winner_edited`/`winner_completed` (row 6 vs. row 7 of
   `specs/conflicts.md` — did the other side's *edit* or *completion* beat the delete; both may be
   true). No aggregate/summary event needed beyond the per-task ones — `resolve` either resurrects
   zero or more named tasks, there is no separate "outcome" to summarize on top of that.
5. **`to_loro.rs::apply`** (line 54, `Op -> Loro` entry point): wrapper/`apply_inner` split (the
   `match` over `OpKind` is real branching). Span: `file = %op.file`, `kind = op_kind_name(&op.kind)`
   — a new leaf `op_kind_name` maps every `OpKind` variant to a static string (`"insert"`,
   `"set_field"`, `"edit_text"`, `"move"`, `"notes_edit"`, `"blank_insert"`, `"blank_remove"`),
   never the op's own payload (line text, edit spans, field values). No separate landed event: the
   span already names what ran, and `apply` unconditionally commits and returns `Ok` on every
   non-error path (the interesting outcome — success/failure — is the `Result` itself, which
   `#[instrument]`'s span already correlates with).
6. **`from_loro.rs::from_batch`** (line 65, `Loro diff -> Vec<Op>` entry point): no split needed —
   the function body is a two-line passthrough into `convert`. `#[instrument(skip_all,
   fields(diffs = batch.len()))]` plus one `log_ops_translated` leaf event (`ops = ops.len()`)
   before the final return.

## Placement

Every leaf log function contains exactly one `tracing` macro call. `lww.rs`, `doc/sync.rs`,
`review.rs`, `resurrect.rs`, `to_loro.rs` get the wrapper+inner split (each has real branching in
the touched function); `export_updates_since` and `from_batch` do not (no branch nests their
return).

## Edge cases

- Nothing here logs a task description, an edit's text, a `ReviewFlag`'s `mine`/`theirs`, or a raw
  `LoroValue` — only ids (`TaskId`, `FilePath`), counts, HLC wall-clock ms + device id, byte
  lengths, and named event/field labels, matching `logging-store`'s precedent.
- `resurrect.rs`'s `TaskChange` struct fields (`deleted`/`completed`/`edited`) stay private to the
  module; `log_task_resurrected` reads them directly, no new pub surface.
- `write_if_newer_inner`'s new `Option<Hlc>` return value is `pub(crate)`-internal shape only — the
  public `write_if_newer(..) -> LoroResult<bool>` signature is unchanged, so no caller anywhere in
  this crate or `txtodo-daemon` needs to change.

## Acceptance

- Every site named in the backlog line emits a span and/or event, no payload/line text/secrets in
  any field.
- `cargo fmt -p txtodo-crdt -- --check`, `cargo clippy -p txtodo-crdt --all-targets -- -D warnings`,
  `cargo test -p txtodo-crdt` green after every commit; no `#[allow]`/`#[expect]` anywhere.
- `.claude/scripts/check-boundaries.sh` clean — `tracing` is an external crate.
- No behaviour change: existing test suite (`tests/sim.rs`, `tests/conflicts.rs`, `*_tests.rs`)
  stays green with the instrumentation compiled in.

## As built (2026-09-16, agent)

Built exactly to the design above, six commits, one per subtask line/site:

1. `77b8d74` — `Cargo.toml` (`tracing = "0.1"`), `Cargo.lock`, `lww.rs`: `write_if_newer`/
   `write_if_newer_inner` wrapper split, span field `key` only, `log_lww_write` (debug:
   `incoming_wall_ms`/`incoming_device`, `existing_wall_ms`/`existing_device` — `None` the first
   time a key is written — and `wrote`).
2. `5345f06` — `doc/sync.rs`: `import`/`import_inner` wrapper split (span `bytes`, event
   `crdt_import_landed` with `applied`); `export_updates_since` got a span (`since_bytes`) plus one
   `crdt_export_updates_since` event (`exported_bytes`), no split needed.
3. `4e63528` — `review.rs`: `detect`/`detect_inner` wrapper split, `log_review_detected` (debug:
   `flags`/`overflowed` counts only).
4. `e12f159` — `resurrect.rs`: `resolve`/`resolve_inner` wrapper split; the old single `loses` bool
   became `mine_loses`/`their_loses`; `log_task_resurrected` fires once per resurrected task
   (`task`, `mine_delete_lost`, `their_delete_lost`, `winner_edited`, `winner_completed`).
5. `9913cd0` — `to_loro.rs`: `apply`/`apply_inner` wrapper split, new `op_kind_name(&OpKind) ->
   &'static str` leaf, span fields `file`/`kind`.
6. `3d9d748` — `from_loro.rs`: `from_batch` got a span (`diffs = batch.iter().count()`, `DiffBatch`
   has no `len()`) and one `crdt_ops_translated` event (`ops`), no split needed.

### Deviations from the plan

- **`DiffBatch::len()` does not exist.** The plan's `batch.len()` field expression does not compile
  — `loro::event::DiffBatch` only exposes `iter()`. Used `batch.iter().count()` instead; confirmed
  by reading `loro-1.16.0`'s own `event.rs` source (`~/.cargo/registry/src/.../loro-1.16.0/src/
  event.rs`) rather than guessing at an API surface.
- **`resurrect.rs`'s per-task log call sits inside the existing `if` branch**, not hoisted out
  branch-free, per the "one-macro-call leaf function per branch" rule (`log_task_resurrected` is a
  single function call inside the branch, and it makes exactly one `tracing::debug!` call) — this
  was the plan's stated approach, not a deviation, but worth confirming it held: `resolve_inner`
  passed clippy's `cognitive_complexity` (budget 10) without needing a second split.
- No other deviations — every site landed exactly as designed, no signature of any `pub`/
  `pub(crate)` function in this crate changed (`write_if_newer`, `import`, `export_updates_since`,
  `detect`, `apply`, `from_batch` all keep their original signatures; only `write_if_newer_inner`'s
  private return type grew a second element), so no caller anywhere in this crate or
  `txtodo-daemon` needed a change.

### Verification

- `cargo fmt -p txtodo-crdt -- --check`: clean, every commit and on final reverification.
- `cargo clippy -p txtodo-crdt --all-targets -- -D warnings`: clean, every commit and on final
  reverification — no `#[allow]`/`#[expect]` anywhere, no `cognitive_complexity` hits on any
  touched function.
- `cargo test -p txtodo-crdt`: 26 unit/lib tests, 10 `tests/conflicts.rs` (1 `#[ignore]`d, expected
  — row 9's fingerprint re-identification, see `specs/conflicts.md`), 3 `tests/sim.rs` (the 20
  fixed-seed sweep + the diverged-device + same-seed-twice cases; the full 1000-seed sweep is
  `just sim`, not part of `cargo test`), all green — including `delete_vs_edit_resurrects_the_task`
  and `delete_vs_complete_keeps_it_completed` (`specs/conflicts.md` rows 6-7, the exact behaviour
  `resurrect.rs`'s new logging observes) — on every commit and on final reverification.
- `.claude/scripts/check-boundaries.sh`: clean — `tracing` is an external crate, never touched by
  the `txtodo-*` edge check.
- `cargo check -p txtodo-daemon`: clean on final reverification — every touched function's public
  signature is unchanged, so no downstream call site needed an edit.
- File-length budget: every touched file landed well under 400 (`lww.rs` 140, `doc/sync.rs` 136,
  `review.rs` 215, `resurrect.rs` 176, `to_loro.rs` 315, `from_loro.rs` 365) — no prose trimming
  needed.
- No in-process subscriber assertion on captured JSON lines: same stance `logging-store`/
  `logging-sync-crate`/`logging-daemon-swallowed-errors` took — this crate deliberately never
  wires `txtodo_telemetry::init`, only the daemon binary does, so proof here is the existing test
  suite staying green with the instrumentation compiled in, not a captured-output test.

### Deliberately out of scope

- Any other `+m11 @observability` backlog line, any `txtodo-daemon`/`txtodo-sync`/`txtodo-store`
  file (all already instrumented by prior tasks), or any file outside `crates/txtodo-crdt/`,
  `tasks/logging-crdt/` and this one root todo.txt line.
- `hydrate.rs`, `notes.rs`, `doc/shadow.rs`, `doc/view.rs` — not named by the backlog line, and
  none of them decides a merge outcome (hydration is a bulk load, the notes doc is a separate M5
  text CRDT with no LWW/review/resurrect machinery, the shadow/view modules are read-side list
  bookkeeping).
