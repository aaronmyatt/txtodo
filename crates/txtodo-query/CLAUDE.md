# txtodo-query

## Purpose
Query language: parse, plan, evaluate over parsed tasks. Plan M6+.

## Public interface
`parse_query`, `plan`, `evaluate`; `--explain` output (not built yet). Today: `matches`,
`matches_terms`, `GOLDEN`, re-exported from `txtodo_core::query` (task `tui-revamp/shared-core`) so
MCP, which may reach this crate but not core, searches exactly as every other client does.

## Invariants
- Pure over core types; no I/O.
- May depend only on: txtodo-core.
