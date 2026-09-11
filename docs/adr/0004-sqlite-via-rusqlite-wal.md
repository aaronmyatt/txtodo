# 0004 — Store the op log in SQLite via rusqlite (bundled), WAL mode

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 004; do not relitigate)

## Context
The op log is append-only, small, and must work on mobile and desktop from one code path. Everything under `.txtodo/` is rebuildable from the files, so the store is a cache with history, not the truth.

## Decision
We will use SQLite through `rusqlite` with the bundled feature and WAL journal mode; schema in `migrations/0001.sql` (plan M3).

## Consequences
- Good: ubiquitous; single file; crash-safe with WAL; works in the iOS app sandbox.
- Bad: bundled SQLite adds compile time; a native build step (plan §0 asks before adding such deps: this ADR is that ask, granted).
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- sled / redb: fewer eyes on crash semantics; no SQL for `txtodo log` queries.
- Flat op files: compaction and indexing become our problem.
