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
