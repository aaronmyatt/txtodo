# txtodo-proto

## Purpose
Protobuf definitions and generated gRPC types.

## Public interface
`ListFiles`, `GetFile`, `Watch`, `Apply`, `History`, `Undo`, `Checkout`, `Health`,
`ListConflicts`, `ResolveConflict`, `GetNotes`, `EditNotes`, `PairOffer`, `PairAccept`,
`PairConfirmSas`, `TokenCreate`, `TokenList`, `TokenRevoke`, `OpLogStream` (plan M7),
`DebugSetGroupKey` (plan M4 `sync-lan-transport`, TEST-ONLY — refused unless the daemon was
started with `TXTODO_TEST_HOOKS=1`).
`HealthResponse` carries four LAN-transport fields (`lan_relay_disabled`/`lan_endpoint_bound`/
`lan_discovery_active`/`lan_group_key_present`) for `txtodo doctor`'s transport-mode line.

## Invariants
- Generated output is a generated artifact (diff-budget exempt, committed alone).
- May depend only on: nothing in the workspace.
