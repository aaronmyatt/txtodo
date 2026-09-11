# txtodo-cli

## Purpose
The `txtodo` binary: todo.sh-compatible commands. Plan M2.

## Public interface
clap commands and aliases exactly as todo.sh; `--json` on listings.

## Invariants
- Direct-file mode is atomic write (temp + rename). Daemon mode when the socket exists.
- May depend only on: txtodo-core, txtodo-proto.
