# docs-global-daemon-update

## Summary

Update design §5/§6 and plan M3 notes for the global-daemon supersession, with the migration
story for existing per-workspace installs.

## As built

`design.md` §5 now describes the workspace registry, one socket per device, `WorkspaceSelector`
routing and the migration story (`--dir` bridge kept, `daemon install` migrates old service units,
`oplog.db` never touched by registration). §6 gets a forward-reference noting the MCP
gateway/tokens stay single-workspace-scoped, deliberately deferred. `implementation-plan.md` M3
gets a superseded-by-ADR-0025 note rather than rewriting the historical bullets (`102cec9`).
