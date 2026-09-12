# Sidecar identity mode with fingerprint assignment via the Hungarian algorithm (design §4.1)

**Status: built and merged 2026-09-13, now the default identity mode** (plan decision 9,
reversed; `docs/questions.md` Q2). This file originally planned it as an M10 opt-in; corrected
below to match what actually shipped, in case a future change needs the real file/module map.

## Goal

Tagged mode puts `id:<ULID>` in every line. Sidecar mode (now the default) puts no tags in the
file: identity lives in a `fingerprints` table in the daemon's own SQLite store, keyed by a
*fingerprint*. After an external edit, the reconciler re-identifies old lines against new ones by
solving an assignment problem — cost = weighted mix of creation-date equality, project/context
overlap, normalised Levenshtein on the description, and position distance — with the Hungarian
algorithm; matches below a confidence threshold become delete+insert (design §4.1, §4.3 step 4).

## As built (corrections to the original plan below)

- **Types**: `crates/txtodo-model/src/identity.rs` — `IdentityMode { Tagged, Sidecar }`,
  `Fingerprint { creation_date, projects, contexts, description_norm, line_index }`,
  `CostWeights` (with `CostWeights::DEFAULT`). Not `crates/txtodo-crdt/` — that crate is CRDT-only;
  identity types are mode-agnostic shapes both `txtodo-cli` and `txtodo-daemon` need, and
  `txtodo-model` already depends on nothing else.
- **Matching**: `crates/txtodo-daemon/src/identity_fingerprint.rs` (`fingerprint_of`, the cost
  function), `identity_levenshtein.rs` (wraps `strsim`, not a hand-rolled Levenshtein),
  `identity_assign.rs` (`assign`, `pathfinding::kuhn_munkres_min` — **not** the `munkres` crate,
  which turned out to have a worse API for this; `pathfinding` is MIT/Apache-2.0, already allowed).
- **Diffing**: `crates/txtodo-daemon/src/reconcile_sidecar.rs` — builds its own `Vec<LineDiff>`
  (a longest-increasing-subsequence over matched pairs decides `Change` vs `Move`; blanks pair
  positionally per anchor-task run) and feeds it into the *same* `delete_pass`/`change_pass`/
  `task_pass`/`blank_pass` tagged mode uses (`reconcile.rs`) — not a separate op-emission path.
  An exact-content prefilter matches byte-identical lines for free before any fingerprint is even
  built, so a single edit in a 10k-line file doesn't fingerprint (or Hungarian-solve) the other
  9,999 lines — see `benches/reconcile.rs`'s `reconcile_sidecar_10k_one_edit` (budget 20 ms).
- **Storage**: `crates/txtodo-store/migrations/0005.sql`'s `fingerprints` table (`file, task,
  status, creation_date, projects, contexts, description_norm, line_index, updated_at,
  retired_at`, PK `(file, task)`) — not `.txtodo/index`. Retiring tombstones the row (never
  deletes it), so a late-arriving peer op against a task since split by a delete+insert still
  finds something that explains it. `Store::{upsert_fingerprint, retire_fingerprint,
  live_fingerprints, tombstoned_fingerprints}`.
- **Mode selection**: `txtodod --identity-mode <tagged|sidecar>` (default `sidecar`); a workspace
  that already has `id:` tags on disk is auto-detected as `Tagged` regardless of the flag — no
  silent mode flip on upgrade (`workspace.rs`'s `load_or_mint_identity_mode`), minted once and
  fixed for the workspace's lifetime, same idiom as `device_id`/`group_id`. CLI config
  `identity_mode = "tagged" | "sidecar"` (`txtodo-cli/src/config.rs`), unset means `Sidecar`.
  The `id:` key name is **not** configurable (no `sid:`/`uid:` alias) — out of scope, not needed:
  sidecar mode never writes any such key at all.
- **Weights** (`docs/questions.md` Q2, `CostWeights::DEFAULT`):
  ```
  cost = 3.0 * date_mismatch(0 or 1)
       + 2.0 * (1 - jaccard(projects))
       + 1.0 * (1 - jaccard(contexts))
       + 6.0 * normalized_levenshtein(description)
       + 1.5 * (position_delta / max_task_count)
  MATCH_THRESHOLD = 5.0   // below the cost of "same everything, description fully rewritten" (6.0)
  ```
  `max_task_count` is the *whole file's* task count, not just the unmatched subset the exact-content
  prefilter leaves for the solver — using the smaller count would let the position term swamp
  every other one whenever only a couple of lines actually changed.

## Edge cases & invariants (verified, not just asserted)

- Honest caveat (design §4.1, not softened): *simultaneous* external edits to the same task on two
  devices may resurrect an edit as a duplicate rather than merge it. No text is ever lost.
- Deleting `.txtodo/` is **not** a full reset for a sidecar workspace the way it is for tagged mode
  (`txtodo-store/CLAUDE.md`'s own invariant now carries this exception): tagged mode recovers ids
  by re-reading `id:` tags from the files; sidecar mode's identity lives only in the fingerprints
  table, so nuking the store loses task-identity continuity for it. Worth a line in user docs.
- Stripping every `id:` tag doesn't apply to sidecar mode (there's nothing to strip) — the
  equivalent stress case is `external_edits_sidecar.rs`'s scenarios: insert/append/delete/reorder/
  CRLF/todo.sh archive, all with zero `id:` tags at any point, against a real `txtodod`.
- A full description rewrite becomes delete+insert, not a forced match — asserted directly
  (`identity_assign.rs`'s and `reconcile_sidecar_tests.rs`'s own unit tests, and
  `external_edits_sidecar.rs`'s daemon-level equivalent).
- Position is one cost term, never the only one.
- Pairing does not yet propagate a group's `identity_mode` to a joining device — `docs/questions.md`
  Q6, raised rather than guessed: unlike `group_id` (a meaningless label safely overwritten), a
  mismatch between what the initiator says and what the joiner's own files already imply has no
  defined resolution, and needs a human decision before it's wired in.

## Tests

- `crates/txtodo-model/src/identity.rs` (round-trip, threshold-below-description-weight const
  assertion), `crates/txtodo-store/tests/identity.rs` (upsert/retire/live/tombstoned),
  `crates/txtodo-daemon/src/identity_assign.rs`/`identity_fingerprint.rs`/`identity_levenshtein.rs`
  (unit), `crates/txtodo-daemon/src/reconcile_sidecar_tests.rs` (8 scenarios, no `DocState`
  round-trip needed), `crates/txtodo-daemon/tests/external_edits_sidecar.rs` (8 scenarios against
  a real `txtodod --identity-mode sidecar`), `benches/reconcile.rs`'s
  `reconcile_sidecar_10k_one_edit` (perf budget).
- Not yet built: a two-device concurrent-edit test asserting the documented duplicate-on-conflict
  tradeoff end to end (needs the CRDT/import machinery, not just single-device reconcile).

## References

- design §4.1 + §4.3 step 4 + §4.7 conflict table (txtodo-design.md); `docs/questions.md` Q2, Q6
- `pathfinding`: https://docs.rs/pathfinding · `strsim`: https://docs.rs/strsim
- Plan file for the implementation session: `floofy-swinging-brooks.md` (not checked into the
  repo — a Claude Code plan-mode artifact; this notes.md is the durable record).
