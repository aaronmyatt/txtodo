# 0018 — The relay is a hosted SaaS, not self-host only

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q5)

## Context
Plan §6.5 defaulted to self-host only, with docs describing how to run `relay/` yourself, pending
a decision on whether the project also runs a public relay. M8 self-hosting docs and M9 push
registration both depend on the answer.

## Decision
We will run the relay as a hosted SaaS that the project owner grants users access to manually,
rather than self-host only. Self-hosting `relay/` may still be documented as an option, but it is
no longer the only supported path.

## Consequences
- Good: users get sync working without standing up their own relay infrastructure; matches the
  drop of LAN transport (docs/questions.md Q11, ADR 0024) — the relay is now the only sync path,
  so it needs to be something most users don't have to operate themselves.
- Bad: introduces an access-granting process (manual, per Q5's answer) and hosting/operational
  responsibility for the project owner that a self-host-only model wouldn't carry.
- Neutral / follow-ups: M9 push registration and M8 docs should describe the hosted path as
  primary; self-hosting docs (if kept) become secondary.

## Alternatives considered
- Self-host only (the prior default): lowest operational burden for the project, highest adoption
  friction for users.
- Fully automated self-serve SaaS signup: no access-granting step, but out of scope for what was
  decided here — manual grants were the explicit choice.
