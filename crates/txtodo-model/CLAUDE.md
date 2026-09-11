# txtodo-model

## Purpose
Workspace tree, TaskId, HLC, Op/OpKind/Principal. Plan M3.

## Public interface
`Hlc`, `Op`, `OpKind`, `Principal`, workspace tree + progress (plan §3.2.5).

## Invariants
- Op model is the one the CRDT will use; do not fork it later.
- May depend only on: txtodo-core.
