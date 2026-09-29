# mcp-version-drift

## Goal

An MCP client starts `txtodo-mcp` over stdio and keeps it for the whole session. Nothing
restarts it when a new build is installed, while `txtodod` is restarted by the next client that
is newer than it (task daemon-auto-upgrade). And `just install-daemon` never installed
`txtodo-mcp` at all, so `txtodo mcp` (which execs the `txtodo-mcp` beside `txtodo`) could run an
old server for good. Decided 2026-09-29 (human): keep the MCP server on demand, not a second
persistent daemon; close the drift instead.

## Design

- `just install-daemon` installs `txtodo-mcp` too.
- `txtodo-mcp` asks the daemon's `Health.version` at start and every 5 minutes; when the daemon is
  newer, it warns once per daemon version: stderr (the MCP client's server log) and a
  `mcp_older_than_daemon` log line. The compare is a plain `major.minor.patch` one; a version it
  cannot parse never warns.

## Known gaps

- The warning lands in the client's MCP server log and txtodo-mcp's own log, not in front of the
  user or the agent; the client still has to be told to reconnect.
