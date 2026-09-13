# 0020 — Priority is one global scale across workspaces

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q7)

## Context
Nothing today compares tasks across files — `crates/txtodo-cli/src/commands/list.rs`'s `sort_key`
is per-file todo.sh order. The universal view (M11), the `pri` operand in `txtodo-query`, and the
multi-workspace MCP gateway all need one answer for what "universal priority" means: one shared
`(A)`–`(Z)` scale, or a scale scoped per workspace with cross-workspace weighting.

## Decision
We will use one global, flat priority scale. `(A)` in `+home` ranks equally with `(A)` in `+work`.
The universal view sorts by `(priority, created, workspace)` and shows the owning workspace in the
breadcrumb. Ties within or across workspaces at the same priority are expected and fine — no forced
tiebreak beyond that sort key. We explicitly reject any implicit cross-workspace weighting (e.g.
"+work outranks +home on weekdays") as a core rule: workspaces stay separate entities the user
switches between explicitly (GUI/TUI/CLI), so "high priority" is always relative to whichever
workspace currently has the user's attention, not a computed global ranking.

## Consequences
- Good: simple, predictable sort; no saved-view or plugin concept needed before M11 ships.
- Bad: a workspace with many `(A)` tasks can crowd a universal view even when another workspace's
  `(B)` tasks are, in the user's head, more urgent right now — there is no core mechanism to say so.
- Neutral / follow-ups: a future cross-workspace aggregation feature (surface top priorities across
  workspaces/lists/projects) is plausible but out of scope; if built, it must be additive — a view
  the user opts into over the query language — not a change to how priority itself is scored.

## Alternatives considered
- Per-workspace scales plus a workspace weight: needs a saved-view or plugin concept (design §8,
  §9) that doesn't exist yet, and bakes a specific weighting policy into core.
