# txtodo-store

## Purpose
SQLite op log, snapshots, projection cache. Plan M3.

## Public interface
open/append/read ops, projections, snapshots; `migrations/0001.sql`.

## Invariants
- Append-only op log. Everything here is rebuildable from the files.
- May depend only on: txtodo-model.
