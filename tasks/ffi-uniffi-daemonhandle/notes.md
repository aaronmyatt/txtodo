# uniffi bindings for core and a DaemonHandle embedding txtodod in-process (plan M9, plan §7)

Design §7: every client is thin — it talks to `txtodod` and renders; none parses the file itself.
On mobile that IPC is in-process: `txtodo-ffi` (a crate stub) exposes uniffi bindings over
`txtodo_core::tokenize` and a `DaemonHandle` that boots the daemon actor inside the app process.
Cite <https://mozilla.github.io/uniffi-rs/>.

## What crosses the FFI boundary

- `tokenize(raw) -> Vec<Span>` with `Span { kind, start, end }`, `kind` one of the nine semantic
  names in plan §3.1 (`priority`, `date`, `completion-marker`, `project`, `context`, `tag-key`,
  `tag-value`, `id-tag`, `text`). Both platforms map those to attributed-string spans; boundaries
  must be byte-identical, so the enum is generated, not hand-copied.
- `DaemonHandle::new(root)`, `apply`, `get`, `subscribe` map 1:1 onto
  `txtodo_daemon::handle::ActorHandle` (cloneable, bounded `ACTOR_MAILBOX_CAP`). The handle owns a
  `tokio` runtime; `drop` shuts it down cleanly.

## Async across the boundary

uniffi exposes `async fn` as Kotlin `suspend` and Swift `async` (https://mozilla.github.io/uniffi-rs/latest/kotlin/async.html
and https://mozilla.github.io/uniffi-rs/latest/swift/async.html). `get`/`apply` are async;
`subscribe` becomes a callback interface so platform code receives `Change` events as a stream
instead of polling.

## Errors

`ActorError` (daemon/handle.rs) maps to a generated `DaemonError` enum — `Mutation`, `State`,
`Store`, `Write`, `Hlc`, `Gone`, `Unsupported`. No variant is dropped; every arm re-raises a
typed error so Kotlin/Swift never see a bare string.

## Acceptance

- `uniffi-bindgen` generates Kotlin + Swift that compile in CI.
- A desktop test asserts `tokenize` through the FFI returns the same spans as `txtodo_core::tokenize` for a known line.
- `DaemonHandle` embedded in a Rust test process applies a mutation, gets bytes back, shuts down cleanly.
