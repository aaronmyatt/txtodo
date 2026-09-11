# Recognise own writes by projection hash and a short-lived expected-write token (plan M3)

Design §4.3 step 2: "Hash the file. If it equals the hash of our own last write, ignore." Plan M3
adds the token "because some filesystems coalesce events". Ref: https://docs.rs/blake3

## Why two checks
- **Hash only** fails when we write twice quickly: the event for write 1 arrives after write 2
  changed `self.hash`, so the bytes of write 1 look foreign. Reconciling them would derive ops that
  revert write 2. The ring of recent hashes covers this.
- **Token only** fails when the FS coalesces our write with a real external edit into one event:
  the bytes differ from anything we wrote, the hash check says "not ours", and we reconcile —
  which is correct. The token never suppresses an event whose bytes we did not produce.

## Shape
```rust
pub struct ExpectedWrites { recent: VecDeque<(Hash, Instant)>, ttl_ms: u64 }   // len <= RECENT_WRITES
impl ExpectedWrites {
    pub fn arm(&mut self, hash: Hash, now: Instant);            // called right before rename()
    pub fn is_ours(&mut self, hash: &Hash, now: Instant) -> bool; // prunes expired, consumes a hit
}
```
Postconditions: `recent.len() <= RECENT_WRITES`; every entry `now - written_at <= ttl`.
The TTL bounds the memory and the window in which a genuinely identical external write could be
mistaken for ours — harmless, since identical bytes produce no ops anyway.

## Order of checks in the actor
1. `bytes == projection`? (cheap length check first) → ignore.
2. `expected.is_ours(hash)` → ignore.
3. reconcile (tasks/daemon-reconciler).
