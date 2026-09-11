# txtodo doctor checks socket, watcher, file-writable, clock, config (plan M3)

Design §5 lists the full doctor set (keystore, relay, ports come in M4–M8). Plan §5: `doctor
--verbose` dumps the last 100 events. Every check is a pure function of injected inputs plus one
I/O probe, so each is unit-testable without a daemon.

## Shape
```rust
pub enum Status { Ok, Warn, Fail }
pub struct Check { pub name: &'static str, pub status: Status, pub detail: String }
pub fn run(env: &Env, paths: &Paths, client: Option<&TxtodoClient>) -> Vec<Check>;   // len == 5, asserted
```
Output order is fixed (socket, watcher, files, clock, config) so scripts can index it.

## Health RPC
The plan's seven RPCs have no way to ask "is the watcher alive". Add
`rpc Health(HealthRequest) returns (HealthResponse { watcher_alive, documents, last_event_age_ms,
started_at })`. Tiny, read-only, and the same RPC the crash-safety and external-edit tests use to
wait for quiescence — one seam, guarded by being read-only.

## Clock check
Reads the newest op's `hlc.wall_ms` via History (limit 1). `now < wall_ms` means the clock went
backwards since the last write: Warn with both values. The HLC itself never regresses (model notes).

## Fix hints
Each Fail carries the command that fixes it: stale socket → `txtodo daemon start`; unwritable
file → the `chmod`; bad config → the path and the TOML error text. Error messages state what was
attempted and with which values (constitution §3).
