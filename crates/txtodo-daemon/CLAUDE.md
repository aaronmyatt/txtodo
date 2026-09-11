# txtodo-daemon

## Purpose
The `txtodod` binary: FileActor per document, watcher, IPC, MCP host. Plan M3.

## Public interface
gRPC server on the socket; `daemon install|start|stop|status`.

## Invariants
- One writer per file (the actor). Clients never touch the file directly.
- May depend only on: txtodo-core, txtodo-query, txtodo-model, txtodo-store, txtodo-crdt, txtodo-sync, txtodo-proto, txtodo-mcp.
