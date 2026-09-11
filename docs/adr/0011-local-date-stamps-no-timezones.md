# 0011 — Dates in the file are the daemon's local date, YYYY-MM-DD, no time zones

- Status: accepted
- Date: 2026-09-11
- Deciders: project owner (plan §1, decision 011; do not relitigate)

## Context
The todo.txt spec defines only `YYYY-MM-DD` creation and completion dates. Other tools (todo.sh, humans) read them as calendar days.

## Decision
We will write `creation_date` and `completion_date` as the daemon's local date at write time, formatted `YYYY-MM-DD`. No timestamps, no offsets, ever, in the file. Precise time lives in the op log (HLC).

## Consequences
- Good: spec-only output (design §2.2 rule 3); byte-identical with todo.sh.
- Bad: two devices in different zones can disagree on the day near midnight; the HLC in the op log is the tie-breaker for sync, not the file.
- Neutral / follow-ups: see the plan milestone that lands it.

## Alternatives considered
- ISO timestamps in the file: not todo.txt.
- UTC dates: surprising to the human whose day it is.
