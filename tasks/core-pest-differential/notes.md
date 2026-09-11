# ABNF-generated second parser, differential testing

Plan M1: "A second parser generated from the ABNF (`abnf` + `pest`, or `abnf-to-pest`) used only in tests,
differentially against the hand-written one on the corpus and on proptest-generated input." Acceptance:
"Differential parser test passes on corpus + 10 000 generated lines."

## Approach
`abnf-to-pest` (https://docs.rs/abnf_to_pest) converts an ABNF rule list to a pest grammar string. The M0
validator already parses `specs/todotxt.abnf` with the `abnf` crate, so the pipeline is:
```
specs/todotxt.abnf --abnf crate--> rulelist --abnf_to_pest--> tests/todotxt.pest (checked in)
```
A test regenerates the `.pest` text and asserts it equals the checked-in file (same pattern as M7's Lezer
grammar CI step). `pest_derive` then builds the second parser from the checked-in grammar at compile time.
Ref: https://pest.rs/book/

## What is compared
- Strict accept/reject per line: `parse_line(raw, Strict).is_ok() == pest_parse(raw).is_ok()`.
- On accept: `completed`, both dates, priority, and the description byte range — extracted from the pest
  pair tree by rule name (`completed`, `date`, `priority`, `description`).
- Lenient mode has no ABNF, so it is not differentially tested; the corpus and proptest cover it.

## Known divergences to handle explicitly (do not paper over)
- Calendar validity: the ABNF accepts `2026-02-30`; the hand-written parser rejects it. Filter such lines
  from the comparison with a predicate that is itself unit-tested, or extend the pest grammar with a
  post-check. Prefer the predicate; keep the ABNF as the pure shape grammar.
- `%s` case-sensitive strings and the `26(…)` repetition must survive conversion; check the generated
  `.pest` by eye once and pin it.
