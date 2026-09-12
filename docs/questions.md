# Open questions

Agents append here when the plan says "stop and ask" (plan §0). Humans answer under the question.
An answered question stays; its Status flips and, if it changed a decision, the ADR is linked.
Append only. Never edit a prior answer; add a dated follow-up.

## Q1 — Do non-managed files inside a `ref:` directory ever sync (attachments)?
- Status: open · Raised: 2026-09-11 (plan §6.1) · Blocks: M5 sync scope, M8 relay payloads
- Default until answered: **no**. Only `todo.txt`, `done.txt`, `notes.md` are synced (plan §3.2.11).
- Answer: Let's limit it to only images, assuming they might be used in the markdown tasks write ups

## Q2 — Sidecar mode: confidence threshold and cost weights for fingerprint re-identification?
- Status: answered 2026-09-13 · Raised: 2026-09-11 (plan §6.2) · Blocks: sidecar identity mode,
  now pulled forward from M10 to be the default (decision reversed, see below)
- Default until answered: none needed before M10. Design §4.1 names the cost terms (creation-date
  equality, project/context overlap, normalised Levenshtein, line distance) without weights.
- Answer: sidecar mode is now the default, not an M10 opt-in (reverses
  `txtodo-implementation-plan.md` decision 9) — a plain, unmanaged todo.txt should work with
  `txtodo` with zero `id:` metadata written into it. Weights, v1/tunable:
  ```
  cost = 3.0 * date_mismatch(0 or 1)
       + 2.0 * (1 - jaccard(projects))
       + 1.0 * (1 - jaccard(contexts))
       + 6.0 * normalized_levenshtein(description)
       + 1.5 * (position_delta / max_task_count)
  MATCH_THRESHOLD = 5.0   // below the cost of "same everything, description fully rewritten" (6.0)
  ```
  Description gets the heaviest weight (strongest human-legible identity signal); the threshold
  sits just under "identical except description fully rewritten" (6.0) so a full rewrite is
  classified delete+insert (a visible duplicate) rather than risking a wrong merge — matches this
  design section's own stated preference for "resurrect as duplicate" over a bad merge. Lives as
  named constants in `crates/txtodo-model/src/identity.rs`; retune there directly, no ADR needed
  for a weight change alone.

## Q3 — Is `rec:` recurrence a core feature or a plugin?
- Status: open · Raised: 2026-09-11 (plan §6.3) · Blocks: M10 plugin host scope
- Default until answered: **plugin** (design §9). The tag still parses in core (specs/todotxt.abnf `rec-tag`).
- Answer: plugin

## Q4 — iOS: accept a Local Network permission prompt for LAN MCP, or make iOS relay-only for agents?
- Status: open · Raised: 2026-09-11 (plan §6.4) · Blocks: M9 iOS MCP transport
- Default until answered: undecided. LAN MCP on iOS needs `NSLocalNetworkUsageDescription` and Bonjour
  service declarations. Ref: https://developer.apple.com/documentation/bundleresources/information-property-list/nslocalnetworkusagedescription
- Answer: scope MCP to only the desktop versions (macOS, Linux, Windows). iOS relay-only for agents. The iOS agent will not be able to use MCP on LAN.

## Q5 — Relay hosting: does the project run a public relay, or self-host only?
- Status: open · Raised: 2026-09-11 (plan §6.5) · Blocks: M8 self-hosting docs, M9 push registration
- Default until answered: **self-host only**; docs describe running `relay/` yourself.
- Answer: not sure what this means, the app should sync across devices, let me know what infra is required to deploy the CRDT setup.

## Q6 — Pairing: what happens when the initiator's and joiner's identity_mode disagree?
- Status: open · Raised: 2026-09-13 (plan `floofy-swinging-brooks.md`, sidecar-identity Phase 2) ·
  Blocks: pairing inheriting identity_mode (a joining device otherwise decides its own, from
  whatever `id:` tags its own files already have — plan decision 3)
- Default until answered: **no propagation yet**. `load_or_mint_identity_mode` runs unconditionally
  in `Workspace::open_with_default_mode`, before pairing ever touches the workspace, so a joiner's
  mode is fixed by the time any pair RPC could adopt one.
- Context: `group_id` propagates today as a plaintext field on `PairOfferResponse`
  (`crates/txtodo-daemon/src/pairing_grpc.rs`), a meaningless random label a joiner blindly
  overwrites via `Workspace::adopt_group_key` — harmless, since nothing about the joiner's own
  state depended on its old value. `identity_mode` is not that: it is derived from real properties
  of the joiner's own files (`Tagged` iff a document already carries an `id:` tag, "no silent mode
  flip on upgrade" — `workspace.rs`'s `load_or_mint_identity_mode` doc) and `DocState`/`reconcile`
  assume it matches what is actually on disk. Blindly overwriting it the way `group_id` is
  overwritten could desync that invariant (e.g. a joiner with already-tagged files told to adopt
  `Sidecar` from the initiator). No wire message for the sealed group key exists yet either
  (`pairing_grpc.rs`'s own module doc: the daemon-to-daemon transport, `sync-lan-transport`,
  doesn't exist) — `PairingRegistry`/`Workspace::adopt_group_key` are a relay seam exercised only
  by `pairing_grpc_tests.rs` standing in for that transport.
- Needs a human decision, not just implementation: does a mode mismatch refuse the pairing outright
  (with what message), does the joiner defer to the initiator only when the joiner's own workspace
  has zero tasks yet (so there is nothing on disk to desync), or something else? Once decided, the
  wire part is small — a new field on `PairOfferResponse`, the same shape as `group_id`'s.
