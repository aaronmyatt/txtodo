# uniffi bindings + in-process DaemonHandle (plan M9, design §7)

## Goal

Plan M9 task 1: `txtodo-ffi` uniffi bindings for `txtodo-core`, plus a `DaemonHandle` API that
embeds `txtodod` in-process. This is the mobile seam from design §7 — every client is thin and
renders; on iOS/Android the daemon runs inside the app process (design §5 "embedded in the app as
a library"), so the FFI exposes the same actor surface the gRPC clients get, minus the socket.

## Design

### The span surface — 9 semantic kinds, byte-identical

Design §7: every client paints using `txtodo_core::tokenize` so token boundaries are identical on
every platform. Plan §3.1 fixes nine semantic names (`priority`, `date`, `completion-marker`,
`project`, `context`, `tag-key`, `tag-value`, `id-tag`, `text`). The FFI collapses core's 12
`TokenKind`s into them: the two dates merge, `Url`/`Whitespace` paint as plain `Text`.

```rust
// crates/txtodo-ffi/src/span.rs — generated, not hand-copied
#[derive(uniffi::Record)]
pub struct Span { pub kind: SpanKind, pub start: u32, pub end: u32 } // byte offsets, UTF-8 safe

#[derive(uniffi::Enum)]
pub enum SpanKind { CompletionMarker, Date, Priority, Project, Context, TagKey, TagValue, IdTag, Text }

impl From<txtodo_core::TokenKind> for SpanKind {
    // CompletionMarker → CompletionMarker
    // CompletionDate | CreationDate → Date          (plan §3.1 "date")
    // Priority → Priority · Project → Project · Context → Context
    // TagKey → TagKey · TagValue → TagValue · IdTag → IdTag
    // Url | Text | Whitespace → Text                (never a gap; tokenize covers [0,len))
}

#[uniffi::export]
pub fn tokenize(raw: String) -> Vec<Span> { txtodo_core::tokenize(&raw).into_iter().map(Span::from).collect() }
```

### DaemonHandle — the in-process daemon

Maps 1:1 onto `txtodo_daemon::handle::ActorHandle` (cloneable, bounded `ACTOR_MAILBOX_CAP = 256`,
`WATCH_CAP = 64` — `crates/txtodo-daemon/src/handle.rs`). The handle owns a `tokio` runtime and
boots the daemon actors for the workspace root; `drop` shuts it down cleanly.

```rust
#[derive(uniffi::Object)]
pub struct DaemonHandle { /* owns tokio::runtime::Runtime + booted FileActors for root */ }

impl DaemonHandle {
    #[uniffi::constructor]
    pub fn new(root: String) -> Result<Arc<Self>, DaemonError>;
    #[uniffi::method]
    pub async fn get(&self, file: String) -> Result<Vec<u8>, DaemonError>;       // → ActorHandle::get
    #[uniffi::method]
    pub async fn apply(&self, mutations: Vec<MutationOp>, principal: Principal) -> Result<Applied, DaemonError>;
    #[uniffi::method]
    pub fn subscribe(&self, listener: Box<dyn ChangeListener>) -> Result<(), DaemonError>; // callback, not polling
}
```

- uniffi `async fn` becomes Kotlin `suspend` and Swift `async`
  (https://mozilla.github.io/uniffi-rs/latest/kotlin/async.html,
  https://mozilla.github.io/uniffi-rs/latest/swift/async.html). `get`/`apply` are async;
  `subscribe` is a callback interface (https://mozilla.github.io/uniffi-rs/latest/kotlin/callback_interfaces.html)
  forwarding `Change` events from the actor's `broadcast` channel.
- `MutationOp`/`Principal` are FFI mirrors of `txtodo_daemon::mutation::Mutation` and
  `txtodo_model::Principal` — the daemon turns them into ops, the client never parses a line.

### Errors — every `ActorError` variant typed

`crates/txtodo-daemon/src/handle.rs::ActorError` has 7 variants: `Mutation`, `State`, `Store`,
`Write`, `Hlc`, `Gone`, `Unsupported`. Map each to a generated `DaemonError` enum; no variant is
dropped, and no bare string crosses the boundary.

## Placement/dependencies

- `crates/txtodo-ffi/src/` gains `lib.rs` (uniffi module), `span.rs`, `handle.rs`, `error.rs`.
  Each ≤ 400 lines, each fn ≤ 60 (budgets.json).
- New deps: `uniffi`, `tokio`, and **`txtodo-daemon`** (for `ActorHandle`/`Mutation`). **Gap:**
  `budgets.json` `allowedDeps["txtodo-ffi"] = ["txtodo-core", "txtodo-query"]` — `txtodo-daemon` is
  not allowed. Fixing that means editing `budgets.json` + a `/setup` re-run; both are frozen
  (`.claude/**`). Stop and ask before the dependency lands.
- `txtodo-daemon` drags the async stack (tokio, tonic, rusqlite-bundled, notify, iroh) onto mobile.
  Native/C-build-step deps trip the plan §0 "stop and ask" rule — surface this explicitly.

## Edge cases & invariants

- Byte-identical boundaries: `tokenize` through the FFI must return the same spans as
  `txtodo_core::tokenize` for the same input — the acceptance test asserts it, so the collapse
  mapping can't silently diverge.
- The owned runtime must not leak: `DaemonHandle::drop` shuts down the runtime; the acceptance test
  drops the handle and asserts the process can exit cleanly (no orphaned reactor).
- `subscribe` is a stream, not polling: platform code receives `Change` events via the callback; on
  disconnect the actor's `Gone` surfaces as `DaemonError::Gone`, not a panic.

## Acceptance

- `uniffi-bindgen` generates Kotlin + Swift that compile in CI.
- Desktop test: `tokenize` via the FFI returns spans equal to `txtodo_core::tokenize` for a known
  corpus line, byte-identical boundaries.
- `DaemonHandle` embedded in a Rust test process applies a mutation, reads bytes back, and drops
  without leaking the runtime.
- `ActorError` round-trips: forcing each of the 7 variants yields a typed `DaemonError`, none
  dropped to a string.

## References

- plan M9 + §3.1 (txtodo-implementation-plan.md), design §5/§7 (txtodo-design.md)
- uniffi: https://mozilla.github.io/uniffi-rs/ · async: https://mozilla.github.io/uniffi-rs/latest/kotlin/async.html · callbacks: https://mozilla.github.io/uniffi-rs/latest/kotlin/callback_interfaces.html
- `crates/txtodo-daemon/src/handle.rs` (ActorHandle/ActorError) — the surface this mirrors.

## Decision: the allowedDeps gap (2026-09-24, human)

Thin daemon-core crate (option A). Split the in-process parts of `txtodo-daemon` (FileActor /
ActorHandle, store, sync) into their own library crate; `txtodod` keeps the gRPC server, socket,
service and LAN listener on top of it, and `txtodo-ffi` depends on the core only. Mobile still
carries SQLite and iroh (sync needs them) but not tonic or the server code. Rejected: `txtodo-ffi`
as a gRPC client of a separate daemon — iOS can't keep one running, and design §5 embeds the
daemon in the app. A new crate boundary needs an ADR first (next line in todo.txt), then a
human-approved `allowedDeps` edit in budgets.json for the new crate and for `txtodo-ffi`.
