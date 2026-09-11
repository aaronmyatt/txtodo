# txtodo-crdt

## Purpose
Loro document per file, ops to/from Loro, reconciler. Plan M4.

## Public interface
`Doc` (Loro-backed), reconcile(external bytes) -> ops, materialise() -> bytes.

## Invariants
- Three-way apply on top of current state, never overwrite.
- Untouched lines materialise byte-identical.
- May depend only on: txtodo-model, txtodo-store, txtodo-core.
