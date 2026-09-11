# Sidecar identity mode with fingerprint assignment via the Hungarian algorithm (plan M10, plan §4.1)

## Goal

Tagged mode (default) puts `id:<ULID>` in every line. Sidecar (purist) mode puts no tags in the
file: IDs live in `.txtodo/index` on each device, keyed by a *fingerprint*. After an external edit,
the reconciler re-identifies `L_old ↔ L_new` by solving an assignment problem — cost = weighted mix
of creation-date equality, project/context overlap, normalised Levenshtein on the description, and
position distance — with the Hungarian algorithm; matches below a confidence threshold become
delete+insert (design §4.1, §4.3 step 4).

## Design

The matching step lives in the reconciler, which currently diffs by `id:` tag (M3/M4). Sidecar mode
replaces that matcher with fingerprint assignment; everything downstream (ops, op log, projection)
is unchanged — the reconciler's external interface stays identical (the M4 constraint).

```rust
// crates/txtodo-crdt/src/identity.rs — new module, owns mode + fingerprint + assignment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdentityMode { Tagged, Sidecar }   // design §4.1; Tagged is the default

pub struct Fingerprint {
    pub creation_date: Option<Date>,        // YYYY-MM-DD, plan §1 decision 11 (no timezones in file)
    pub projects: BTreeSet<String>,         // +project set, order-insensitive
    pub contexts: BTreeSet<String>,         // @context set, order-insensitive
    pub description: String,                // normalised (trimmed, lowercased) before distance
}

pub struct CostWeights {                    // plan §6 Q2 — threshold + weights, answered in docs/questions.md first
    pub creation_date_eq: f64,              // reward for equal creation dates
    pub project_overlap: f64,               // Jaccard overlap of +project sets
    pub context_overlap: f64,               // Jaccard overlap of @context sets
    pub levenshtein: f64,                   // normalised edit distance on description (0..1)
    pub position: f64,                      // |old_line_index - new_line_index|, line order is data (rule 5)
    pub match_threshold: f64,               // cost above this → delete + insert, not a match
}

/// Rectangular cost matrix solved by the Hungarian algorithm (Kuhn–Munkres, O(n³)).
/// Ref: https://docs.rs/munkres
pub fn assign(old: &[TaskState], new: &[TaskState], w: &CostWeights) -> Assignment;
pub struct Assignment { pub matched: Vec<(usize, usize)>, pub inserted: Vec<usize>, pub deleted: Vec<usize> }
```

- The cost matrix is `old.len() × new.len()`: `cost(i,j) = w.creation_date_eq * (date_i != date_j)
  + w.project_overlap * (1 - jaccard(proj_i, proj_j)) + w.context_overlap * (1 - jaccard(ctx_i,
  ctx_j)) + w.levenshtein * lev_norm(desc_i, desc_j) + w.position * |i - j|`. Pad to square for the
  solver; mask pad rows/cols to ∞ so they never match.
- Matches whose cost exceeds `w.match_threshold` are *not* taken — the old line becomes a delete and
  the new line an insert (a fresh ULID), exactly as design §4.1 states.
- `.txtodo/index` maps `Fingerprint → TaskId` and lives in `txtodo-store` (a new table in the op-log
  SQLite, alongside `ops`/`snapshots`). In sidecar mode the `id:` is derived at ingest, stored only
  in the index, never written to the file. The index is rebuildable from the files (fingerprints are
  derivable), so deleting `.txtodo/` stays the nuclear reset.
- Mode is a workspace config flag (`identity_mode = "tagged" | "sidecar"`); tagged remains default.
  The `id:` key name is configurable (`sid:`, `uid:`, …) per design §4.1, so `id:` never collides.

## Placement/dependencies

- `crates/txtodo-crdt/src/identity.rs` (new), `txtodo-store` schema migration (new `index` table),
  a config flag, and the reconciler matcher swap in the existing reconcile path. New dep `munkres`
  (pure Rust, no native build) needs human sign-off + `cargo deny` pass.
- Depends on `crdt-loro-state` (Loro-backed `TaskState` the matcher consumes) and the reconciler's
  existing external-edit flow. `crates/txtodo-core` and `Cargo.toml` remain frozen.

## Edge cases & invariants

- Honest caveat (design §4.1, do not soften): *simultaneous* external edits to the same task on two
  devices may resurrect an edit as a duplicate rather than merge it. It will never lose text — the
  no-loss invariant holds (every description substring survives somewhere).
- Strip all `id:` tags in vim → fingerprint re-identification (design §4.7 row) must reassign by
  content, not position, and no line may vanish.
- Below-threshold: two near-identical descriptions must not cross-match (delete+insert instead);
  above-threshold false-positives are worse than a duplicate.
- Position is one cost term, never the only one — a pure position match would silently swap
  identities on any insert/delete above.
- Weights + threshold are plan §6 Q2: answer in `docs/questions.md` *before* tuning; keep them named
  constants with units in the name (e.g. `POSITION_WEIGHT`, `MATCH_THRESHOLD`), not magic numbers.

## Acceptance

- Strip every `id:` tag from a 10 k-line file externally: the reconciler re-identifies via
  fingerprint, `txtodo log` shows no spurious delete+insert beyond genuine edits, and no text is lost.
- Insert a line at the top: position cost does not steal identities; every surviving line keeps its
  stored `TaskId` across the edit.
- Two near-identical descriptions with different creation dates score below threshold → delete+insert.
- Simulator: sidecar-mode runs reuse the M4 sync simulator (random ops + external edits + partitions)
  and assert convergence + no-loss with the same zero-failure bar.

## References

- design §4.1 + §4.3 step 4 + §4.7 conflict table (txtodo-design.md); plan M10 + §6 Q2
- `munkres`: https://docs.rs/munkres · Levenshtein: https://docs.rs/levenshtein (normalise to 0..1)
- [../crdt-loro-state/notes.md](../crdt-loro-state/notes.md) (the `TaskState` the matcher consumes)
