# Reference relay binary in relay/: ciphertext blobs by group + device, APNs/FCM wake-ups (plan M8)

Design §4.5 ("Relay" row) and Appendix B: "The relay. It stores blobs it can't read and forwards
pushes. That is all it will ever do." §4.6: the relay is untrusted, sees only ciphertext and routing
metadata. New top-level `relay/` crate — a standalone Rust binary, no `txtodo-*` deps.

## A dumb mailbox

- Store opaque blobs keyed by `(group_id, device_id)`. Never parse, never decrypt; the bytes are the
  ciphertext envelope from [sync-crypto-envelope](../sync-crypto-envelope/notes.md).
- Routing metadata only: group id, device id, envelope length, stored-at. The store's blob type is
  `Vec<u8>` with no structure — that is where "relay cannot distinguish op types" is enforced.

## Wake-ups: seam now, send later

APNs/FCM tokens register at M9, so M8 ships the queue + trait, not the push calls:

```rust
trait Push { fn wake(&mut self, device: &DeviceId, payload: &[u8]) -> Result<(), PushError>; }
```

A stored-blob write enqueues one wake for the target device; a no-op impl logs and drops. M9 swaps
in APNs/FCM behind the same trait.

## Bounds

`MAX_BLOB_SIZE`, `MAX_BLOBS_PER_DEVICE`, `MAX_RETENTION_DAYS`, `MAX_WAKEUP_QUEUE` — all asserted,
plus a per-group request cap so the relay isn't free scratch space. Storage: SQLite or a filesystem
dir; config via env/flags (port, data dir, retention). No strong device auth (all ciphertext).

## Tests

- A blob written under `(g1, d1)` returns only for `(g1, d1)`; `(g1, d2)` and `(g2, d1)` get nothing.
- Blobs stored byte-for-byte; nothing parses them.
- Size and per-device caps enforced; one write enqueues exactly one wake-up.
