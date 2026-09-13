# 0016 — rec: recurrence stays a plugin, not a core feature

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q3)

## Context
Design §9 already named `rec:` recurrence as a plugin, with the tag itself still parsing in core
(`specs/todotxt.abnf` `rec-tag`) so non-plugin clients don't choke on it. Q3 asked whether that
default should hold or be pulled into core ahead of M10's plugin host.

## Decision
We will keep `rec:` recurrence as a plugin, confirming design §9 as written. Core continues to
parse the tag without acting on it; recurrence behavior (spawning the next instance, etc.) ships
only with the M10 plugin host.

## Consequences
- Good: keeps core's surface small; recurrence logic isn't load-bearing for M1–M9 milestones.
- Bad: no recurrence behavior exists until M10 lands, even though the tag is visible earlier.
- Neutral / follow-ups: no scope or schedule changes — this ADR exists to record the confirmation,
  not because anything moved.

## Alternatives considered
- Pull `rec:` into core now: no plugin host exists yet to justify the core complexity ahead of need.
