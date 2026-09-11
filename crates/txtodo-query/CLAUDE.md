# txtodo-query

## Purpose
Query language: parse, plan, evaluate over parsed tasks. Plan M6+.

## Public interface
`parse_query`, `plan`, `evaluate`; `--explain` output.

## Invariants
- Pure over core types; no I/O.
- May depend only on: txtodo-core.
