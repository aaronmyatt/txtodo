# ratatui TUI: vim keys, sync indicator, conflict review (plan M10, design §7)

## Goal

`crates/txtodo-tui/` (already an empty M0 crate — no workspace edit) becomes a thin ratatui client.
Design §7: "TUI | ratatui | vim keys, live sync indicator, conflict review" and "All clients are thin:
they talk to `txtodod` over local IPC and render. None of them parse the file themselves." The TUI is
Rust, so it links `txtodo-core` directly for `tokenize` (identical token boundaries — design §7) and
talks to the daemon over the socket via `txtodo-proto`. It never opens the file.

## Design

Transport = ADR 0010 socket `<workspace>/.txtodo/txtodod.sock` carrying gRPC (`tonic`), the same seam
`desktop-tauri-shell` uses. The TUI reuses the generated `TxtodoClient` from the shared proto
(`crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`) and adds two M10 RPCs there (see proto list below).

```rust
// crates/txtodo-tui/src/daemon.rs — thin gRPC wrapper, mirror of desktop's DaemonClient
pub struct Daemon {
    inner: TxtodoClient<Channel>,   // tonic-build generated from the shared proto
    sock: PathBuf,                  // <workspace>/.txtodo/txtodod.sock
}
impl Daemon {
    pub async fn connect(sock: &Path) -> Result<Self, DaemonError>;   // connect_lazy + bounded retry
    pub async fn get_file(&mut self, path: &str) -> Result<Vec<u8>, DaemonError>;
    pub async fn watch(&mut self, paths: &[String]) -> Result<tonic::Streaming<Change>, DaemonError>;
    pub async fn apply(&mut self, req: ApplyRequest) -> Result<ApplyResponse, DaemonError>;
    pub async fn sync_status(&mut self) -> Result<SyncStatusResponse, DaemonError>;
}

// crates/txtodo-tui/src/paint.rs — the only place token colours live
pub fn paint_line(raw: &str) -> ratatui::text::Line<'static>;  // tokenize -> styled spans
pub fn span_to_tui(s: &txtodo_core::Span, raw: &str) -> ratatui::text::Span<'static>;
```

- `paint_line` maps `txtodo_core::TokenKind` to the §3.1 semantic colours (`priority`, `date`,
  `completion-marker`, `project`, `context`, `tag-key`, `tag-value`, `id-tag`, `text`), completed
  lines muted + struck through, `id:` tags hidden behind a toggle.
- vim keys (concrete map): `j/k` move · `gg/G` first/last · `dd` delete (`Apply` Delete) · `Space`
  toggle complete (`Apply` Complete) · `i`/`a`/`A` open the line editor (`Edit`/`Add`) · `Enter` open
  the `ref:` detail view (recursive `GetFile`) · `/` search (local substring over the fetched lines —
  `txtodo-tui` has no `txtodo-query` dep) · `r` conflict review pane · `s` toggle sync indicator ·
  `:q` quit.
- Sync indicator (`s`): renders `SyncStatusResponse` — peers with `lag_ms` and `pending_ops`. Driven
  by the new `rpc SyncStatus` (M10) refreshed on a 1 s tick plus every `Watch` event.
- Conflict review (`r`): a pane listing tasks flagged `needs_review` (M4: surfaced via `Watch`'s
  `Change`); `mine`/`theirs`/`merged` resolves by writing the chosen text as a new op through
  `Apply { mutations: [Edit] }`, which clears the local flag (M4 §4).

### M10 proto additions (shared with editor-plugins)

In `crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`, added once, consumed by both M10 clients:

```protobuf
service Tokenizer {                            // "tokenize over the socket" (non-Rust clients)
  rpc Tokenize(TokenizeRequest) returns (TokenizeResponse);
  rpc Complete(CompleteRequest) returns (CompleteResponse);
}
message SyncStatusRequest {}
message SyncStatusResponse { repeated Peer peers = 1; uint32 pending_ops = 2; }
message Peer { string device = 1; int64 last_seen_ms = 2; int64 lag_ms = 3; }
```

(`SyncStatus` serves this task; `Tokenize`/`Complete` serve editor-plugins and are declared here so the
proto change is one commit — see [../editor-plugins/notes.md](../editor-plugins/notes.md).)

## Placement/dependencies

- `crates/txtodo-tui/` exists (M0's 12 crates); no workspace-member edit, no frozen `Cargo.toml`
  change. `allowedDeps["txtodo-tui"] = ["txtodo-core", "txtodo-proto"]` — already satisfied.
- New external deps need human sign-off + `cargo deny check` pass: `ratatui`, `crossterm`, `tokio`
  (full), `tonic`, `prost` (client), `hyper-util`, `tower` — the same set `txtodo-cli` already carries
  for the socket dial. `txtodo-proto` grows `SyncStatus`/`Tokenizer` messages (not frozen).
- The TUI is a client only: it must NOT link `txtodo-store`, `txtodo-crdt`, `txtodo-sync` (boundary
  check). All state comes from `GetFile` + `Watch`. No frozen path is touched by this task.

## Edge cases & invariants

- Daemon absent: `connect` fails → show a one-line "daemon not running — run txtodo daemon start"
  banner and exit non-zero; the TUI never spawns the daemon (the desktop shell does, the TUI does not).
- `Watch` drop mid-stream: reconnect with bounded retry then re-`get_file` to re-baseline (same rule as
  desktop); a stale cursor must never paint a line that no longer exists.
- `Apply` on a stale `TaskRef` (line number + id disagree) is rejected by the daemon — surface the
  error inline, re-baseline, retry once (never loop unbounded; cap at 3).
- Invariant (assert the negative): every buffer change is an `Apply`; the TUI never writes the file
  itself; `get_file` bytes are only ever painted, never mutated.

## Acceptance

- Launch with the socket present: renders the file with token colours matching the §3.1 semantic
  names; completed lines struck; `id:` hidden until toggled.
- `dd` then `Space` then `i`+save each round-trip through `Apply` and repaint via `Watch` — an
  integration test starts a real `txtodod` on a temp dir and drives `Daemon` directly.
- An external edit (append a line out-of-band) appears in the TUI within the Watch debounce without a
  manual refresh.
- Conflict review: seed two concurrent `EditText` ops (M4 sim) → task listed in the `r` pane; choose
  `merged` → a new `Edit` op lands and the flag clears.
- `s` shows a peer and its lag after a second daemon pairs on loopback; `pending_ops` reaches 0 after
  convergence (≤ 2 s, M4 budget).
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test -p txtodo-tui` green.

## References

- plan M10 (txtodo-implementation-plan.md), design §7 (txtodo-design.md)
- https://docs.rs/ratatui · https://docs.rs/crossterm · https://docs.rs/tonic
- sibling: [../editor-plugins/notes.md](../editor-plugins/notes.md) (shared Tokenize/Complete RPCs)

## SyncStatus RPC design (2026-09-17, this session — resuming the 2026-09-13 partial ticket)

Root cause of the 4 still-open lines below: everything in this task except the `s` indicator's
live data is done and tested (vim keys, edit, conflicts, paint, app loop — see root todo.txt's
consolidated line, `ref:tui`). Closing this out needs one new RPC, threaded through 3 crates
(proto → daemon → tui), each its own slice-fence session — this note exists so none of that design
work has to be re-derived per session.

### Proto (`crates/txtodo-proto/proto/txtodo/v1/txtodo.proto`)

```protobuf
message SyncStatusRequest {
  WorkspaceSelector workspace = 1;
}

message SyncStatusResponse {
  message Peer {
    string device = 1;   // ULID text, same form as Device.id
    int64 lag_ms = 2;    // now_ms - last_seen_ms; 0 if never seen (mirrors Device.last_seen_ms's
                          // own "0 = never contacted" convention, not a sentinel to special-case)
  }
  repeated Peer peers = 1;
  uint64 pending_ops = 2;
}
```
Add `rpc SyncStatus(SyncStatusRequest) returns (SyncStatusResponse);` to the `Txtodo` service,
next to `DeviceList`. Regenerate via whatever this crate's own build.rs/committed-output convention
is (check `txtodo-proto/CLAUDE.md` first — other RPCs' additions in this repo commit generated
code, not just the .proto source).

### Daemon (`crates/txtodo-daemon/src/`, new `sync_status_grpc.rs` or folded into an existing file
— check `server.rs`'s dispatch list and file-length budgets before picking)

`devices_grpc.rs::device_list_impl`/`to_pb` is the exact template: same `ws.identity_store()
.lock()...list_devices()`, same `now_ms = ws.clock().now_ms()`, same `self_device = ws.device()`
pattern. `peers` = every row where `!removed_at_ms.is_some() && device != self_device`, mapped to
`Peer { device: row.device.ulid().to_string(), lag_ms: now_ms.saturating_sub(row.last_seen_ms
.unwrap_or(now_ms)) as i64 }`.

`pending_ops` — **no per-peer ack/synced-seq is persisted anywhere today** (checked: `devices`
table has `relay_node_id`/`last_seen_ms`/`key_epoch`/`removed_at_ms`, nothing that says "what seq
has peer X acked" — real per-peer convergence tracking would need new persistence, a bigger change
than this RPC). Chosen approximation, honestly scoped rather than faked: count of local ops (every
tracked file, same `ws.paths()` + `store.newest(path, ...)` pattern `activity.rs::newest_rows`
already uses) whose `op.hlc.wall_ms` is newer than the **oldest** peer's `last_seen_ms` — i.e. "ops
committed since we last heard from our most-out-of-touch peer." `0` peers ⇒ `0` pending
(nothing to be pending against). **Known limitation to document in the handler's own doc comment,
not hide**: this over-counts once a peer reconnects and acks everything (still shows non-zero
until its `last_seen_ms` itself advances past those ops' timestamps) — real per-peer seq tracking
is the correct fix, flagged as a future improvement, not attempted here.

### TUI (`crates/txtodo-tui/src/daemon.rs`, `app.rs`)

`daemon.rs`'s own module doc already flags exactly where this slots in: a `Daemon::sync_status()`
method mirroring `list_conflicts`/`resolve`'s shape, called on a 1s tick in `app.rs`'s event loop
(`ui/sync.rs` needs zero changes — it already renders whatever's in `AppState.sync`, per its own
module doc "recommended build order step 4"). Map `pb::SyncStatusResponse` → `state::SyncSnapshot`/
`PeerStatus` (both already exist, built ahead of the wire type per `state.rs`'s own comment).

### Test lines still blocked after SyncStatus lands

- `01M2B4ZWPK65Q4EHNK1GGD31MP` (needs_review integration test): separately blocked on seeding a
  real flag needing `txtodo_store` directly, out of `txtodo-tui`'s `allowedDeps` — not this RPC's
  problem, still open after SyncStatus ships.
- `01M2B4ZWPKR2YNEF33NVATY9EZ` (two-loopback-daemon `s` test): unblocks once SyncStatus ships —
  do this one alongside the TUI wiring stage, not as a separate session.
