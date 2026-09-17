# cli-workspace-commands

## Summary

`txtodo workspace add|remove|list`; `--dir` and cwd resolve which registered workspace the CLI
targets on the global daemon.

## As built

- **Proto**: `WorkspaceAdd`/`WorkspaceRemove`/`WorkspaceList` RPCs, registry-scoped, with no
  `WorkspaceSelector` field of their own (`5546661`).
- **Daemon**: `GlobalService` delegates to three new `WorkspaceCatalog` methods over the existing
  `WorkspaceRegistry`; `remove` drops it from `open` too (`6f71770`).
- **CLI**: `client::select()` falls back to the true global daemon when no per-dir socket exists,
  attaching a real `Path` selector to every RPC (previously hardcoded `None` for the whole
  session); new `workspace` subcommand; `bundle_import`'s selector now rides real metadata
  (`a52c586`).

Two new real-daemon integration tests prove a plain `txtodo add` with zero per-dir socket reaches
the global daemon end to end, not direct-file mode.
