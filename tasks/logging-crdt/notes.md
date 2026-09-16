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
