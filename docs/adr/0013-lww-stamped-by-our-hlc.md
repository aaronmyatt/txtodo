# 0013 — LWW registers carry our HLC stamp in the value

<!-- MADR-lite — https://adr.github.io/madr/ -->
- Status: accepted
- Date: 2026-09-12
- Deciders: project owner (crdt-loro-doc plan, decision A)

## Context
Design §4.2 resolves last-writer-wins map keys by our hybrid logical clock (wall + counter +
device id). Loro resolves its own map-key conflicts by an internal Lamport clock and peer id —
a different clock we cannot substitute. Letting Loro adjudicate would make design §4.2's ordering
rule, and the clock-skew guard that leans on it, stop meaning what they say.

## Decision
We will store `{ value, hlc }` in every LWW register (a plain key in the task's `LoroMap`) and
write only when the incoming HLC beats the stored one. Loro's own ordering becomes a tiebreak we
never rely on; our Hlc is the merge rule, readable in one function (`lww.rs`).

## Consequences
- Good: design §4.2 stays honest; the clock-skew guard (`model-hlc-skew-guard`) stays meaningful;
  the merge rule is visible in one function instead of hidden in a dependency.
- Bad: every LWW read unwraps a struct; the stamp duplicates data Loro already tracks internally.
- Neutral / follow-ups: the device id breaks exact ties deterministically; `lww.rs` owns the
  encode/decode and the write-if-newer predicate.

## Alternatives considered
- Let Loro's Lamport order be the truth and demote our HLC to a display/ordering aid: rejects
  design §4.2, makes the skew guard cosmetic, and hides the merge rule in a dependency we cannot
  audit against our own clock.
