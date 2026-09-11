# CLI switches to daemon mode when the socket exists, with --no-daemon fallback (plan M3)

Plan §2: `txtodo-cli` depends on core and proto; it talks to the daemon and falls back to direct
file mode. Slice rule: the CLI never imports `txtodo-daemon`; the socket is the boundary.
Ref: tonic over UDS https://github.com/hyperium/tonic/blob/master/examples/src/uds/client.rs

## Mode selection
```rust
pub enum Mode { Daemon(TxtodoClient), Direct }
pub fn select(paths: &Paths, no_daemon: bool) -> Result<Mode, ClientError>
```
Socket path is `paths.dir.join(".txtodo/txtodod.sock")` (ADR 0010). Exists + connects → Daemon.
Missing → Direct. Exists + connect error → `ClientError::SocketRefused { path }`. A stale socket
after a crash is a real state worth surfacing; `txtodo doctor` explains it and `daemon start` fixes it.

## Command mapping
Line numbers stay the user-facing id (CLI CLAUDE.md). The CLI resolves a line number to the
`TaskId` from the `GetFile` bytes it just fetched and sends both; the daemon rejects the mutation
if the line at that number no longer has that id (`ApplyError::Stale`) — the "someone edited in
between" race becomes a clear message instead of the wrong line changing.

## Output parity
Formatting functions in `commands::list` take bytes, not a path, so both modes share them.
The parity harness (`tests/todosh_parity.rs`) gains a `mode` axis: every scenario runs
direct and daemon; files are byte-identical either way. Runtime roughly doubles (~6 s); acceptable.

## Async in a sync CLI
`tokio` current-thread runtime built in `main` only when `Mode::Daemon` is chosen — direct mode
stays runtime-free so `--no-daemon` keeps its M2 startup time.
