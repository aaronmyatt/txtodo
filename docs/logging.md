# Logging: jq recipes

Companion to `.claude/logging.md` (schema, span table, sink matrix, no-secrets rule). This file is
just the runnable part: jq recipes for reconstructing one session's timeline from the real rotated
JSON log files under `<workspace>/.txtodo/logs/*.log.*` (or `$XDG_DATA_HOME/txtodo/logs/` for a
global-mode daemon). Every recipe below was run against a real captured log file produced by the
actual `txtodo`/`txtodod` binaries (see `tasks/logging-docs/notes.md` for the exact commands); none
of this is hypothetical JSON.

`jq` docs: <https://jqlang.org/manual/>

## 1. Sort every line in one file by timestamp

Files are append-only per process but not guaranteed byte-ordered across restarts (log rotation,
retries). `jq -s` slurps the whole file into an array first so `sort_by` can reorder it.

```sh
jq -s 'sort_by(.timestamp) | .[]' .txtodo/logs/txtodo.log.2026-09-16
```

## 2. Filter to one event name

```sh
jq -c 'select(.fields.message == "cli.mode_selected")' .txtodo/logs/txtodo.log.2026-09-16
```

Real output (3 lines from a real `txtodo add`/`do`/`do` run under `TXTODO_LOG=debug`):

```json
{"service":"txtodo","timestamp":"2026-09-16T12:47:07.927548Z","level":"DEBUG","fields":{"message":"cli.mode_selected","mode":"direct","reason":"no_socket_found"},"target":"txtodo::client"}
```

## 3. Merge every service's log file into one sorted session timeline

The most useful recipe for reconstructing what happened across a pairing/sync round, or any moment
where more than one binary was running (e.g. `txtodo` + `txtodod` sharing one workspace's
`.txtodo/logs/` directory, or two `txtodod` processes' separate directories concatenated by hand).
`service` (stamped by `txtodo-telemetry`'s `stamp.rs` on every line) is what lets you tell the merged
lines apart again.

```sh
jq -s 'sort_by(.timestamp) | .[] | {timestamp, service, level, event: .fields.message}' \
  .txtodo/logs/*.log.*
```

Real output, merging a `txtodo` CLI file and a `txtodod` daemon file from the same directory:

```json
{"timestamp":"2026-09-16T12:47:07.927548Z","service":"txtodo","level":"DEBUG","event":"cli.mode_selected"}
{"timestamp":"2026-09-16T12:47:07.950534Z","service":"txtodo","level":"DEBUG","event":"cli.mode_selected"}
{"timestamp":"2026-09-16T12:47:07.973559Z","service":"txtodo","level":"DEBUG","event":"cli.mode_selected"}
{"timestamp":"2026-09-16T12:47:36.172193Z","service":"txtodod","level":"INFO","event":"starting"}
{"timestamp":"2026-09-16T12:47:36.172316Z","service":"txtodod","level":"INFO","event":"daemon_ready"}
{"timestamp":"2026-09-16T12:47:36.173258Z","service":"txtodod","level":"INFO","event":"workspace_closed"}
```

## 4. Reconstruct one session across two paired devices

For a real two-daemon pairing/converge flow (the shape `crates/txtodo-daemon/tests/
logging_flow_sequence.rs` asserts programmatically — see `.claude/logging.md`'s span table for the
locked-in event order: `pairing_joiner_group_key_adopted` → `lan_shared_session_started` →
`lan_link_hello_accepted` → `commit_done`), point the same merge recipe at both devices' log
directories and grep down to the span/event names of interest:

```sh
jq -s 'sort_by(.timestamp) | .[]' \
  device-a/.txtodo/logs/txtodod.log.* device-b/.txtodo/logs/txtodod.log.* \
  | jq -c 'select(.fields.message as $m | ["pairing_joiner_group_key_adopted","lan_shared_session_started","lan_link_hello_accepted","commit_done"] | index($m))'
```

## 5. Everything one span emitted (span stack included)

An event fired inside an entered span carries `span` (innermost) and `spans` (full stack) — useful
for tying an event back to the `rpc{method,workspace}` or `cli.command{name,mode}` call it happened
under:

```sh
jq -c 'select(.span != null) | {timestamp, message: .fields.message, span: .span}' \
  .txtodo/logs/txtodo.log.2026-09-16
```
