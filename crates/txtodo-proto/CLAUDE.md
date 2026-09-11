# txtodo-proto

## Purpose
Protobuf definitions and generated gRPC types.

## Public interface
`ListFiles`, `GetFile`, `Watch`, `Apply`, `History`, `Undo`, `Checkout`.

## Invariants
- Generated output is a generated artifact (diff-budget exempt, committed alone).
- May depend only on: nothing in the workspace.
