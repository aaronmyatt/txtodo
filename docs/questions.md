# Open questions

Agents append here when the plan says "stop and ask" (plan §0). Humans answer under the question.
An answered question stays; its Status flips and, if it changed a decision, the ADR is linked.
Append only. Never edit a prior answer; add a dated follow-up.

## Q1 — Do non-managed files inside a `ref:` directory ever sync (attachments)?
- Status: open · Raised: 2026-09-11 (plan §6.1) · Blocks: M5 sync scope, M8 relay payloads
- Default until answered: **no**. Only `todo.txt`, `done.txt`, `notes.md` are synced (plan §3.2.11).
- Answer: _(human)_

## Q2 — Sidecar mode: confidence threshold and cost weights for fingerprint re-identification?
- Status: open · Raised: 2026-09-11 (plan §6.2) · Blocks: M10 sidecar identity mode
- Default until answered: none needed before M10. Design §4.1 names the cost terms (creation-date
  equality, project/context overlap, normalised Levenshtein, line distance) without weights.
- Answer: _(human)_

## Q3 — Is `rec:` recurrence a core feature or a plugin?
- Status: open · Raised: 2026-09-11 (plan §6.3) · Blocks: M10 plugin host scope
- Default until answered: **plugin** (design §9). The tag still parses in core (specs/todotxt.abnf `rec-tag`).
- Answer: _(human)_

## Q4 — iOS: accept a Local Network permission prompt for LAN MCP, or make iOS relay-only for agents?
- Status: open · Raised: 2026-09-11 (plan §6.4) · Blocks: M9 iOS MCP transport
- Default until answered: undecided. LAN MCP on iOS needs `NSLocalNetworkUsageDescription` and Bonjour
  service declarations. Ref: https://developer.apple.com/documentation/bundleresources/information-property-list/nslocalnetworkusagedescription
- Answer: _(human)_

## Q5 — Relay hosting: does the project run a public relay, or self-host only?
- Status: open · Raised: 2026-09-11 (plan §6.5) · Blocks: M8 self-hosting docs, M9 push registration
- Default until answered: **self-host only**; docs describe running `relay/` yourself.
- Answer: _(human)_
