# txtodo-proto

## Purpose
Protobuf definitions and generated gRPC types.

## Public interface
`ListFiles`, `GetFile`, `Watch`, `Apply`, `History`, `Undo`, `Checkout`, `Health`,
`ListConflicts`, `ResolveConflict`, `GetNotes`, `EditNotes`, `PairOffer`, `PairAccept`,
`PairConfirmSas`, `PairAwaitPeer` (plan M4 `sync-pairing`'s LAN wiring pass: the initiator polls
this — `PairResult.sas` empty means "still waiting" — to learn a joiner's `PairAccept` has reached
it over the real LAN transport and show a real SAS, without the RPC itself ever blocking),
`TokenCreate`, `TokenList`, `TokenRevoke`, `OpLogStream` (plan M7),
`DeviceList`, `DeviceRemove` (plan M4 tasks/sync-device-remove),
`DebugSetGroupKey` (plan M4 `sync-lan-transport`, TEST-ONLY — refused unless the daemon was
started with `TXTODO_TEST_HOOKS=1`).
`WorkspaceSelector` (ADR 0025, task `daemon-global-socket`, M11): a `oneof workspace_id/path`
carried by every RPC request message (added as their last field, `workspace`) so the one global
`txtodod` knows which registered workspace to route a call to. `GetNotes` changed shape from a
bare `TaskRef` request to `GetNotesRequest { task, workspace }` so the selector has somewhere to
live without polluting `TaskRef`'s many nested uses (`Complete`/`Edit`/`Move`/`Delete`/
`MoveToEnd`/`ResolveRequest`/`NotesEditRequest`/`RefDirRequest` all still carry a bare `TaskRef`).
`BundleImport`'s request type is fixed to the streamed `BundleChunk`, so its selector rides in
request metadata (`x-txtodo-workspace-selector-bin`, the encoded `WorkspaceSelector` bytes) the
same way its passphrase already does.
`HealthResponse` carries `key_store_backend` (plan M4 tasks/sync-keystore) and four LAN-transport
fields (`lan_relay_disabled`/`lan_endpoint_bound`/`lan_discovery_active`/`lan_group_key_present`,
fields 8-11 — `key_store_backend` already held field 7) so `txtodo doctor` can report both.
`lan_relay_disabled` is a real runtime flag as of plan M8 `sync-relay-enable`/ADR 0026 (true iff no
relay is configured), not the field-8-era hardcoded `true`. Fields 12-13, added the same task:
`relay_url` (the configured relay, empty when off) and `relay_last_outcome` (human-readable result
of the most recent relay bind/accept/connect attempt, empty until one has happened) — design §5's
"doctor: relay reachability".

## Invariants
- Generated output is a generated artifact (diff-budget exempt, committed alone).
- May depend only on: nothing in the workspace.
