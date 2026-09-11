# Reference relay binary: ciphertext blobs by group+device, APNs/FCM wake-ups (plan M8, design §4.5)

## Goal

A new top-level `relay/` crate — a single standalone Rust binary that is the "Relay" row of design
§4.5: store *encrypted* op blobs per device group, forward push wake-ups (APNs/FCM) to mobile.
Appendix B is the contract: "The relay. It stores blobs it can't read and forwards pushes. That is
all it will ever do." §4.6: the relay is untrusted; it sees only ciphertext + routing metadata.

## Design

No `txtodo-*` dependencies — the relay must be unable to accidentally import the crypto or parse an
op. The store's blob type is `Vec<u8>` with no structure, which is where "cannot distinguish op
types" is enforced.

```rust
// relay/src/store.rs — opaque blobs keyed by (group_id, device_id)
pub struct Store { /* sqlite or fs dir */ }
impl Store {
    pub fn put(&mut self, g: GroupId, d: DeviceId, blob: Vec<u8>) -> Result<(), StoreError>;
    pub fn get(&mut self, g: GroupId, d: DeviceId) -> Result<Vec<Vec<u8>>, StoreError>;
    pub fn list(&mut self, g: GroupId) -> Result<Vec<DeviceId>, StoreError>;
}
// routing metadata ONLY: group id, device id, envelope length, stored-at. Never parse/decrypt.

// relay/src/push.rs — seam now, send later (APNs/FCM tokens register at M9)
pub trait Push { fn wake(&mut self, device: &DeviceId, payload: &[u8]) -> Result<(), PushError>; }
pub struct NoopPush;   // M8 impl logs + drops; M9 swaps APNs/FCM behind the same trait
```

- A stored-blob write enqueues exactly one wake for the target device (`MAX_WAKEUP_QUEUE`).
- Bounds, all asserted: `MAX_BLOB_SIZE`, `MAX_BLOBS_PER_DEVICE`, `MAX_RETENTION_DAYS`,
  `MAX_WAKEUP_QUEUE`, plus a per-group request cap so the relay isn't free scratch space.
- Config via env/flags: listen port, data dir, retention; refuse to run without an explicit data
  dir (never guess). Storage: SQLite or a filesystem dir.
- No strong device auth — everything is ciphertext, so the relay needs none (design §4.6). Any
  S3/WebDAV endpoint also works as a dumb relay (design §4.5), which is why the HTTP surface stays
  dumb: put/get/list + wake, nothing else.

## Placement/dependencies

- New top-level crate `relay/` → root `Cargo.toml` workspace member (**frozen — ask**); `Cargo.lock`
  updated (generated artifact, committed alone).
- Deps: an HTTP/QUIC server (e.g. `axum` or iroh's relay node) + `sqlite`/std fs. Zero `txtodo-*`
  deps by construction — assert this in a test (the crate cannot compile against `txtodo-core`
  even by accident).

## Edge cases & invariants

- Blobs stored byte-for-byte; nothing parses or decrypts. Envelope length is metadata, content is not.
- Retention sweep removes blobs older than `MAX_RETENTION_DAYS`; per-device cap evicts oldest first.
- One write = exactly one wake-up; the wake queue is bounded and drained, never grown unboundedly.

## Acceptance

- A blob written under `(g1, d1)` is returned only for `(g1, d1)`; `(g1, d2)` and `(g2, d1)` get nothing.
- Blobs round-trip byte-for-byte; size and per-device caps enforced; one write enqueues exactly one
  wake-up for the target device.

## References

- design §4.5 (Relay row), §4.6 (trust), Appendix B; plan M8.
- [sync-crypto-envelope](../sync-crypto-envelope/notes.md) (what a blob is) ·
  https://docs.rs/iroh (relay-node impl reused for M9 wake-ups).
