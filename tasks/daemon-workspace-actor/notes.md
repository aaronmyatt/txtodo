# daemon-workspace-actor

## Summary

Nest today's `FileActor` set under a `WorkspaceActor` keyed by workspace id, each with its own
store, op log, CRDT and sync `Link`.

## As built

The nesting (`FileActor` set/store/op-log/CRDT keyed by `WorkspaceId`) is already real, delivered
by `daemon-workspace-registry`'s `WorkspaceCatalog` (`HashMap<WorkspaceId, OpenedWorkspace>`, each
a fully isolated `Workspace`) — proven by `workspace_catalog_tests.rs` and
`tests/global_socket.rs`.

The "sync Link" half of this line was wrong as originally scoped: ADR 0025's actual decision is
that `WorkspaceActor` must **not** own a private `Link` — sync is one shared, device-set-scoped
`Link` multiplexed by `workspace_id` per ADR 0021, not one per workspace.

Split into the two real remaining pieces:

- `daemon-device-set-identity` — move device id/keystore/group key out of per-`Workspace`.
- `daemon-shared-sync-link` — the multiplexed `Link` itself (depends on the first).

Both are their own separately-tracked tasks.
