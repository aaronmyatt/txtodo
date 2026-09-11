# 0002 — Use Loro as the CRDT engine

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 002; do not relitigate)

## Context
Line order is data (design §2.2 rule 5), descriptions merge at character level, notes are text. The engine needs a movable list, a text type and a map.

## Decision
We will use Loro. We will wrap it behind a `Doc` trait only when a second implementation exists; until then `txtodo-crdt` uses Loro directly.

## Consequences
- Good: native movable list so a move is a move, not delete+insert; text and map types included.
- Bad: one vendor; Loro's on-disk format is ours to migrate if we ever swap.
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- Automerge: a list move is delete+insert, which loses concurrent edits to the moved line.
- Yrs: no movable list; weaker Rust-first story.
