# Tauri 2 shell talking gRPC to txtodod and spawning it if absent (plan M7, plan §7)

## Goal

`apps/desktop/` is a Tauri 2 shell. The Rust side talks gRPC to `txtodod`; if the socket is
absent it spawns the daemon. All clients are thin (design §7) — the daemon owns the files, the
CRDT, and sync; the UI renders and calls into it.

## Design

- Daemon local IPC: unix domain socket (named pipe on Windows) carrying gRPC over `tonic`
  (design §5). Schema is the one shared proto `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`
  (ADR 0006). The desktop client is generated from that proto, not hand-rolled — one schema, no drift.
  Ref: https://docs.rs/tonic/latest/tonic/
- Spawn: reuse the same start path as `txtodo daemon start`
  (`tasks/daemon-service-files`), or spawn the binary directly. Detect the socket first; only spawn
  when absent; never run two daemons. Resolve the socket path from the same config the CLI reads.
- Connect with `tonic::transport::Channel::connect_lazy` so app startup does not block on daemon
  startup; wrap with a bounded retry and a timeout.
- Tauri commands expose a narrow typed surface (`ListFiles`, `GetFile`, `Watch`, `Apply`,
  `History`) to the Svelte frontend. No file I/O in the frontend; every read/write goes through the
  daemon.
- Bounds: `MAX_CONNECT_RETRIES`, `connect_timeout_ms`, `spawn_timeout_ms` as named constants. A
  "daemon absent / disconnected" state surfaces as a reconnect banner, never a crash.

## Acceptance

- Fresh install, no daemon: launching the app spawns `txtodod`; the first `ListFiles` succeeds
  within the startup budget.
- A daemon already running (e.g. from the CLI) is reused, not duplicated.
- Killing the daemon under the app shows a reconnect banner and recovers without a restart.

Refs: plan M7 (txtodo-implementation-plan.md), design §5 and §7 (txtodo-design.md),
Tauri 2 https://v2.tauri.app/
