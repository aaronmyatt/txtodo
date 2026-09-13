# txtodo-proto

## Purpose
Protobuf definitions and generated gRPC types.

## Public interface
`ListFiles`, `GetFile`, `Watch`, `Apply`, `History`, `Undo`, `Checkout`, `Health`,
`ListConflicts`, `ResolveConflict`, `GetNotes`, `EditNotes`, `PairOffer`, `PairAccept`,
`PairConfirmSas`, `TokenCreate`, `TokenList`, `TokenRevoke`, `OpLogStream` (plan M7),
`DeviceList`, `DeviceRemove` (plan M4 tasks/sync-device-remove),
`DebugSetGroupKey` (plan M4 `sync-lan-transport`, TEST-ONLY — refused unless the daemon was
started with `TXTODO_TEST_HOOKS=1`).
`HealthResponse` carries `key_store_backend` (plan M4 tasks/sync-keystore) and four LAN-transport
fields (`lan_relay_disabled`/`lan_endpoint_bound`/`lan_discovery_active`/`lan_group_key_present`,
fields 8-11 — `key_store_backend` already held field 7) so `txtodo doctor` can report both.

## Invariants
- Generated output is a generated artifact (diff-budget exempt, committed alone).
- May depend only on: nothing in the workspace.
