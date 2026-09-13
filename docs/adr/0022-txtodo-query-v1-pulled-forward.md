# 0022 — Pull txtodo-query v1 forward into M11, matching design §8 in full

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q9)

## Context
Three consumers each need "filter tasks by some criteria" ahead of M10, when `txtodo-query` was
originally planned: the universal view's filter bar (todo 140), MCP `todo_search` (currently
substring-only, no query index), and a cross-workspace `txtodo ls`. Left alone, each keeps growing
its own ad-hoc filter (ABSTRACTIONS.md 2026-09-13 already flags three such filters), while
`crates/txtodo-query/src/lib.rs` stays two lines.

## Decision
We will build `txtodo-query` now, pulled forward from M10 into M11, as one shared `Query::parse` /
`Query::matches(&Task)` / `SortKey`, called by the CLI, MCP, and desktop instead of each growing
its own filter. The grammar matches design §8 in full — including relative dates and
`--explain` — rather than shipping a pared-down v1 subset; §8 is frozen as written.

## Consequences
- Good: one filter/sort implementation instead of three diverging ones; unblocks the universal
  view filter bar, a real `todo_search`, and cross-workspace `ls` together.
- Bad: larger scope pulled into M11 than a stripped subset would have been — relative-date parsing
  and `--explain` ship now rather than being deferred.
- Neutral / follow-ups: `crates/txtodo-query/src/lib.rs` moves from a two-line stub to the real
  parser/matcher/sort-key implementation this milestone.

## Alternatives considered
- Ship a v1 subset now (operands `done pri +project @context text key:value`, connectives
  `and or not`, no relative dates), full §8 later: less up-front scope, but risks a second grammar
  migration once relative dates and `--explain` are added.
- Wait for M10 as originally planned: lets the three ad-hoc filters keep diverging in the meantime.
