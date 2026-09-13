# 0019 — Drop Tagged identity mode; Sidecar is the only identity mode

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q6)

## Context
ADR 0009 introduced `id:<ULID>` tagging as identity; ADR 0015 made sidecar (fingerprint) identity
the default while keeping `Tagged` alive for documents that already carry an `id:` tag. That left
an open question (docs/questions.md Q6): what happens when a pairing initiator and joiner disagree
on `identity_mode`? A 2026-09-13 follow-up shipped a stopgap — refuse pairing on a non-empty
mismatch — without a real policy, because `identity_mode` (unlike the meaningless `group_id`
label) is derived from real properties of a device's own files and `DocState`/`reconcile` assume
it matches what's on disk.

## Decision
We will drop `Tagged` identity mode entirely. `Sidecar` becomes the only `identity_mode` in the
system. This dissolves Q6 rather than answering it: with one mode, an initiator/joiner mismatch is
no longer representable, so there is no mismatch policy left to design.

## Consequences
- Good: removes an entire class of pairing bugs (identity_mode desync) at the source instead of
  policing it at the pairing boundary; one mode is simpler to reason about end to end.
- Bad: dead code to remove — the mismatch-policy refusal check and `PairOfferResponse.identity_mode`
  wire field (`crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`) built for the stopgap, and the
  `Tagged` branch throughout `load_or_mint_identity_mode`/`DocState`/`reconcile`
  (`crates/txtodo-daemon/src/workspace.rs`) and `txtodo pair`'s refusal path
  (`crates/txtodo-daemon/src/pairing_grpc.rs`). Any document that was already relying on explicit
  `id:` tags for identity now gets re-identified by fingerprint instead.
- Neutral / follow-ups: supersedes ADR 0009 (via ADR 0015) — sidecar-only, not sidecar-default.

## Alternatives considered
- Keep both modes, decide a mismatch policy (e.g. defer to initiator only when the joiner's
  workspace is empty): still leaves a permanent desync risk between what `identity_mode` says and
  what's actually on disk whenever the policy's assumptions don't hold.
- Keep `Tagged` as the default, drop `Sidecar`: reverses ADR 0015's adoption-friction rationale.
