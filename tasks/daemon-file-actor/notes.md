# FileActor per synced document, single writer, owning state and projection hash (plan M3)

Design §4.3 "Concurrency inside the daemon": one reconciler actor per file, single writer, so the
file is never written by two paths at once. Client mutations go through the same actor as (later)
CRDT ops. Ref: tokio tasks and channels https://docs.rs/tokio/latest/tokio/sync/index.html

## Shape
```rust
pub enum ActorMsg {
    ExternalChange,                                              // from the watcher
    Apply { mutations: Vec<Mutation>, principal: Principal, reply: oneshot::Sender<Result<Applied, ApplyError>> },
    Get { reply: oneshot::Sender<FileContents> },
    Undo { steps: u16, reply: oneshot::Sender<Result<Applied, ApplyError>> },
    Checkout { at: Hlc, reply: oneshot::Sender<Result<Vec<u8>, CheckoutError>> },
    Subscribe { reply: oneshot::Sender<broadcast::Receiver<Change>> },
}
```
One `select!`-free loop: `while let Some(msg) = rx.recv().await { handle(msg) }` — bounded by the
mailbox; the loop ends when every sender drops. `handle` dispatches with an exhaustive `match`.

## Invariants (become assertions)
- `hash == blake3(projection)` after every message (postcondition).
- `materialise(&state) == projection` — the in-memory state and the last written bytes agree.
- Only `write_projection` touches the path; it is `fn(&mut self)` and private.
- `state.len() <= MAX_LINES_PER_FILE` (named const with the unit).

## Write path
Same temp-file-beside-target + fsync + rename as `txtodo-cli/src/store.rs` (design §2.2 rule 7).
Slices never import each other, so copy it and log the second copy in `ABSTRACTIONS.md`
(the ledger already flags the read-mutate-write pattern; append, never edit).

## Injected
`Clock`, `Store` (opened by the caller), the path. No `std::env`, no globals (stack.md idioms).
