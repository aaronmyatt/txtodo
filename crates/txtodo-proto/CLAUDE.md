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
`WorkspacePendingOffers`, `WorkspaceAcceptOffer`, `WorkspaceDeclineOffer` (task
`daemon-workspace-identity-agreement`, M11): the human accept/decline surface for a peer's
control-channel workspace offer (offer/accept, first-registrant-wins). Device-level like
`WorkspaceAdd`/`WorkspaceRemove`/`WorkspaceList` — never a `WorkspaceSelector`. `PendingWorkspaceOffer`
carries `offering_device`/`workspace_id` as ULID text (ADR 0025's existing convention) plus a
human-readable `name` and `offered_at_ms`; `WorkspaceAcceptOffer` returns the same `WorkspaceInfo`
message `WorkspaceAdd` does.
`WorkspaceSelector` (ADR 0025, task `daemon-global-socket`, M11): a `oneof workspace_id/path`
carried by every RPC request message (added as their last field, `workspace`) so the one global
`txtodod` knows which registered workspace to route a call to. `GetNotes` changed shape from a
bare `TaskRef` request to `GetNotesRequest { task, workspace }` so the selector has somewhere to
live without polluting `TaskRef`'s many nested uses (`Complete`/`Edit`/`Move`/`Delete`/
`MoveToEnd`/`ResolveRequest`/`NotesEditRequest`/`RefDirRequest` all still carry a bare `TaskRef`).
`BundleImport`'s request type is fixed to the streamed `BundleChunk`, so its selector rides in
request metadata (`x-txtodo-workspace-selector-bin`, the encoded `WorkspaceSelector` bytes) the
same way its passphrase already does.
`Mutation.Replace { base_hash, contents }` (field 7): a whole-document compare-and-swap, refused
(`FAILED_PRECONDITION`, nothing written) unless the document's hash is still `base_hash`. It is
the CLI's fallback for a diff no other mutation can express, and must be alone in its `Apply`.
`Mutation.RequireBase { base_hash }` (field 8): the same hash check as a leading precondition on
an ordinary `Apply` batch, for batches that address lines by number with no `id:` to check (sidecar).
`HealthResponse` carries `key_store_backend` (plan M4 tasks/sync-keystore) and four LAN-transport
fields (`lan_relay_disabled`/`lan_endpoint_bound`/`lan_discovery_active`/`lan_group_key_present`,
fields 8-11 — `key_store_backend` already held field 7) so `txtodo doctor` can report both.
`lan_relay_disabled` is a real runtime flag as of plan M8 `sync-relay-enable`/ADR 0026 (true iff no
relay is configured), not the field-8-era hardcoded `true`. Fields 12-13, added the same task:
`relay_url` (the configured relay, empty when off) and `relay_last_outcome` (human-readable result
of the most recent relay bind/accept/connect attempt, empty until one has happened) — design §5's
"doctor: relay reachability".

Additive fields and messages, 2026-09-20 (all default to "absent" on an older peer):
`WorkspaceInfo.load_state`/`load_error` (`WorkspaceLoadState`: the daemon binds first and opens
workspaces in the background, task `daemon-early-bind`); `HealthResponse.workspaces_*` totals plus
`relay_node_id`/`relay_bound` (task `cli-relay-node-id`); `MigrateIdentityResponse.paired_peers`;
`Mutation.MoveBefore` (same-file reorder, task `mcp-move-reorder`); the read-only `Lint` RPC (task
`mcp-hygiene-parity`, so a client that may not link txtodo-core still gets the CLI's findings).

## Invariants
- Generated output is a generated artifact (diff-budget exempt, committed alone).
- May depend only on: nothing in the workspace.
