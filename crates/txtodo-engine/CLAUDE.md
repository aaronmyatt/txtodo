# txtodo-engine

## Purpose
The merge engine, moved out of `txtodo-daemon` on 2026-10-17 (ADR 0042, task `web-pwa`) so the
daemon and the browser peer run the same code. Builds for `wasm32-unknown-unknown`.

## Public interface
- `state::DocState`: one document's entries from its ops (`apply`), rendered to exact bytes
  (`to_bytes`), with ghost placements (ADR 0033), text history (ADR 0034, `text_history`) and the
  LWW fields (`fields`). `state_error::StateError` says why an op does not fit.
- `mutation`: a client's intent (`Mutation`) → ops; `mutation_add`, `mutation_moves`,
  `mutation_reopen` hold the variants that would not fit one file.
- `reconcile` / `reconcile_sidecar`: a file edited outside txtodo → ops, with ids kept across the
  edit by `identity_assign` (Kuhn-Munkres), `identity_fingerprint` and `identity_levenshtein`.
- `apply::apply_leniently` and `sync_park`: a peer's batch, tolerant of order inside one commit;
  ops naming a task not here yet wait (bounded).
- `duplicates` (ADR 0032), `contents` (what `GetFile` answers), `op_set_hash` (ADR 0035 digest),
  `stamp_floor` (ADR 0040), `local_date` (ADR 0011), `unified_diff`, `notes_state`, `textedit`,
  `fastid`, `id_strip`, `conflict_row`. `Hash` and `hash_of` (blake3 of a document's bytes).
- Tests: unit tests per module plus the converge suites (`state_converge*`, `state_order_tests`'s
  exhaustive orders, `text_history_converge_tests`, `state_goldens`' exact bytes).

## Invariants
- Pure: no I/O, no tokio, no gRPC, no `Instant`. The one clock read is `local_date`'s date; the
  one store read is `op_set_hash`.
- Proto conversions stay in the daemon's gRPC layer (`conflicts_grpc.rs` builds the wire duplicate group).
- Same input ops → same bytes, whatever order they arrive in; the converge tests pin it.
- May depend only on: txtodo-core, txtodo-model, txtodo-store (external: tracing, blake3, jiff,
  pathfinding, strsim).
