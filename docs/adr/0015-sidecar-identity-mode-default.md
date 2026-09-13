# 0015 — Sidecar identity mode is the default, not an M10 opt-in

- Status: superseded by 0019
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q2)

## Context
ADR 0009 made `id:<ULID>` tagging the only identity mode through M1–M7, deferring fingerprint-based
sidecar identity to M10. That means a plain, unmanaged `todo.txt` cannot be synced by `txtodo`
without first being rewritten with `id:` tags — a real adoption barrier this decision reverses.

## Decision
We will make sidecar (fingerprint) mode the default: `txtodo` works against a zero-`id:`-metadata
todo.txt from the start. Sidecar re-identification costs are:
```
cost = 3.0 * date_mismatch(0 or 1)
     + 2.0 * (1 - jaccard(projects))
     + 1.0 * (1 - jaccard(contexts))
     + 6.0 * normalized_levenshtein(description)
     + 1.5 * (position_delta / max_task_count)
MATCH_THRESHOLD = 5.0
```
living as named constants in `crates/txtodo-model/src/identity.rs`. Description carries the
heaviest weight as the strongest human-legible identity signal; the threshold sits just under the
cost of "identical except description fully rewritten" (6.0), so a full rewrite resolves as
delete+insert (a visible duplicate) rather than risking a wrong merge.

## Consequences
- Good: zero-friction adoption for existing plain-text todo.txt users; matches the design's own
  stated preference for "resurrect as duplicate" over a bad merge.
- Bad: sidecar matching is heuristic — a pathological edit (rewritten description, moved position,
  changed projects/contexts, all at once) can still misfire; weights are tunable, not proven.
- Neutral / follow-ups: weight changes alone don't need a new ADR — retune the constants directly.
  `Tagged` mode still exists for documents that already carry an `id:` tag (see ADR 0009); it is
  no longer the default.

## Alternatives considered
- Keep `Tagged` as the only mode until M10 (status quo, ADR 0009): blocks adoption by anyone with
  an existing plain-text file.
- Ship both modes with equal weight, no default: pushes an identity-mode choice onto every new
  user before they've written a single task.
