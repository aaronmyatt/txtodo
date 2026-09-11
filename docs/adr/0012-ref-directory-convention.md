# 0012 — A task's notes and sub-list live in a sibling directory named by a ref: slug

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 012; do not relitigate)

## Context
A task that needs more than one line must not get a richer line (design §2.6). Notes and sub-lists must sync, nest, and stay usable by todo.sh.

## Decision
We will use the `ref:<slug>` tag naming a directory beside the file, holding optional `todo.txt`, `done.txt`, `notes.md`, created lazily, synced by walking the tree. The normative rules are `specs/ref-directories.md` (mirror of plan §3.2).

## Consequences
- Good: still plain files; recursion for free; other tools see an inert tag.
- Bad: orphan directories need `txtodo prune --orphans`; slug rules must be fuzzed for traversal (plan §5).
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- Richer line syntax: breaks the spec-only rule.
- Notes in the op log or a sidecar DB: not a text file; not the truth.
