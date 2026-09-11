# 0009 — Task identity is an id:<ULID> tag; sidecar mode is deferred to M10

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 009; do not relitigate)

## Context
todo.txt lines have no identity; sync needs one that survives inserts, reorders and external edits (design §4.1).

## Decision
We will stamp every task with `id:<ULID>` (26 chars, time-sortable), hidden by clients by default, key name configurable. Tagged mode is the only mode for M1–M7; sidecar (fingerprint) mode is an M10 item.

## Consequences
- Good: robust under any concurrent edit; spec-compliant metadata other tools ignore.
- Bad: ~30 bytes of noise per line for purists; a stripped tag falls back to fingerprinting (M10).
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- Sidecar fingerprints first: simultaneous external edits can duplicate a task; ship the robust path first.
- Line numbers as identity: breaks on any insert above.
