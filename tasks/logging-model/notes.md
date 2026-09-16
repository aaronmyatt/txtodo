# txtodo-model: instrument the HLC decision sites (root todo.txt line 36)

## Goal

`crates/txtodo-model/src/hlc.rs` has zero `tracing` calls. Root todo 36 names three sites:
`tick` (line 115, send rule), `merge` (line 133, receive rule), and `Skew::check` (line 82, the
clock-skew guard) — the guard is the one place a genuine decision (is the peer within tolerance)
is currently made silently. Adds `tracing = "0.1"` (event/span emission only — this low-level
crate never wires a subscriber, that is `txtodo-daemon`'s job) and instruments exactly these three
functions. Never changes observable behaviour — every new code path is a `tracing` call and
nothing else.

## Design

### Severity convention (same as `logging-crdt`/`logging-store`/`logging-sync-crate`)

Every site here is a routine, expected clock decision (there is no failure branch distinct from
the existing `Result`/`Skew` return values, which callers already see and decide their own
severity on — the HLC doc comment already says "Behind is safe, callers warn"). Every new event
is `debug`.

### Sites

1. **`Skew::check`** (line 82): thin `#[instrument(skip_all)]` wrapper around a renamed
   `check_inner` (`#[instrument]` on top of the existing three-way `if`/`else if`/`else` risks the
   `cognitive_complexity` budget). `log_skew_checked` (leaf, one `debug!` call, branch-free —
   `match` only computes `(label, lead_ms, lag_ms)` values, the macro call happens once after)
   records `peer_ms`, `local_ms`, `skew` (`"ok"`/`"ahead"`/`"behind"`), and `lead_ms`/`lag_ms`
   (`None` unless that branch fired). `check` is shared by `merge`, the sync `Hello` handshake and
   `txtodo doctor` (existing doc comment, line 69), so logging here covers all three callers with
   one instrumentation site.
2. **`Hlc::tick`** (line 115, send rule): thin wrapper around `tick_inner`, same reason. `log_tick`
   (leaf, one `debug!` call, branch-free via `Result::ok`/`is_err`) records `now_ms`, the
   resulting `wall_ms`/`counter` (`None` on `Overflow`), and `overflow: bool`.
3. **`Hlc::merge`** (line 133, receive rule): thin wrapper around `merge_inner` (real branching:
   the skew-refusal early return, the four-way counter-selection `match`). `log_merge` (leaf, one
   `debug!` call, branch-free) records `remote_wall_ms`, `remote_device`, `now_ms`, the resulting
   `wall_ms`/`counter` (`None` on `Err`), and `refused: bool`. Does not re-decide or re-log the
   skew classification — `Skew::check`'s own event (site 1) already fires inside `merge_inner` and
   covers "was this peer within tolerance."

## Placement

Every leaf log function (`log_skew_checked`, `log_tick`, `log_merge`) contains exactly one
`tracing` macro call, matching `crates/txtodo-crdt/src/lww.rs`'s `log_lww_write` pattern (just
landed, commit `77b8d74`). All three touched functions get the wrapper+`*_inner` split — each has
real branching in the touched body.

## Edge cases

- Nothing here logs a task, op, file path, or line of task text — `hlc.rs` never sees any of
  those (module doc: "no I/O and no clock in this crate"). Only wall-clock ms, HLC counters,
  device ids, byte-free classification labels, and booleans/counts.
- `tick_inner`/`merge_inner`/`check_inner` are private (`pub(crate)` not even needed) — the public
  `tick`/`merge`/`Skew::check` signatures are unchanged, so no caller anywhere in the workspace
  needs to change.
- `merge_inner` still calls `Skew::check` (now itself instrumented) for the ahead-refusal guard —
  no duplicate skew-decision logging is added inside `merge`.

## Acceptance

- All three named sites emit a span and/or event; no payload/line text/secrets in any field.
- `cargo fmt -p txtodo-model -- --check`, `cargo clippy -p txtodo-model --all-targets -- -D
  warnings`, `cargo test -p txtodo-model` green after every commit; no `#[allow]`/`#[expect]`
  anywhere.
- No behaviour change: existing `hlc_tests.rs` suite (unit + proptest) stays green with the
  instrumentation compiled in.

## As built (2026-09-16, agent)

Built exactly to the design above, one commit (small slice — one crate, three sites, all under
the diff-line budget):

- `Cargo.toml`: added `tracing = "0.1"` (event/span emission only, no subscriber wired).
- `hlc.rs`: `Skew::check`/`check_inner` wrapper split, `log_skew_checked` (debug: `peer_ms`,
  `local_ms`, `skew` label, `lead_ms`/`lag_ms` — `None` unless that branch fired). `Hlc::tick`/
  `tick_inner` wrapper split, `log_tick` (debug: `now_ms`, `wall_ms`/`counter` — `None` on
  overflow, `overflow: bool`). `Hlc::merge`/`merge_inner` wrapper split, `log_merge` (debug:
  `remote_wall_ms`, `remote_device`, `now_ms`, `wall_ms`/`counter` — `None` on `Err`,
  `refused: bool`).

### Deviations from the plan

- None. All three sites landed with the wrapper/`*_inner` split as designed; every leaf log
  function makes exactly one `tracing` macro call, branch-free (values computed first via `match`
  or `Result::ok`/`is_err`, the macro call fires once after).

### Verification

- `cargo fmt -p txtodo-model -- --check`: clean.
- `cargo clippy -p txtodo-model --all-targets -- -D warnings`: clean — no `cognitive_complexity`
  hits on `check`, `tick`, or `merge` (each now a thin wrapper), no `#[allow]`/`#[expect]`
  anywhere.
- `cargo test -p txtodo-model`: 33 tests green, including every `hlc_tests.rs` case (unit +
  proptest) — no behaviour change from the instrumentation.
- `.claude/scripts/check-boundaries.sh`: clean — `tracing` is an external crate.
- `cargo check -p txtodo-crdt -p txtodo-store -p txtodo-daemon`: clean — no touched function's
  public signature changed (`tick`, `merge`, `Skew::check` all keep their original signatures;
  only the new private `*_inner` siblings are new), so no downstream caller needed a change.

### Deliberately out of scope

- Any other `+m11 @observability` backlog line, or any file outside `crates/txtodo-model/hlc.rs`
  and its `Cargo.toml`, `tasks/logging-model/`, and this one root todo.txt line.
- A `cargo clippy -p txtodo-store` failure (`projections.rs` cognitive-complexity) surfaced once
  from an unrelated hook run; confirmed clean on `main` before this change and untouched by it —
  not this task's to fix.
