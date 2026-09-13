# 0023 — Two real txtodod processes per agent session are allowed, per-session

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q10)

## Context
Five todo lines (8, 9's integration half, 10, 11, 20) were each tagged "off-limits this session"
because they need two real `txtodod` processes running at once, and it was unclear whether that
was a standing rule agents must never cross or a caution specific to individual sessions. Left
undecided, agents kept skipping them and the five lines never closed.

## Decision
We will treat this as a per-session concern, not a standing rule. Agents may run two real
`txtodod` processes within a session, via a `TwoDaemons` test fixture (ABSTRACTIONS.md
2026-09-13) that is bounded by a timeout and kills its processes on drop, so no stray process or
port survives a test run.

## Consequences
- Good: todo lines 8, 9 (integration half), 10, 11, 20 stay agent work and can actually close once
  the fixture exists, instead of perpetually bouncing to a human.
- Bad: relies on the fixture's timeout/kill-on-drop discipline being correct; a bug there could
  leak processes or ports on a developer's machine.
- Neutral / follow-ups: no `just sync-e2e` human-run recipe or `@human` retagging is needed for
  these five lines.

## Alternatives considered
- Standing rule (never two real daemons in an agent session): requires a human-run `just sync-e2e`
  recipe and retagging the five lines `@human`, permanently moving this class of test out of agent
  reach — appropriate only if the real worry is stray sockets/ports on the human's own machine,
  which the bounded/kill-on-drop fixture already addresses.
