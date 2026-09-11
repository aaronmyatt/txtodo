# Web PWA with the WASM core in a Web Worker, OPFS storage, WebSocket sync (plan M10, plan §7)

## Goal

`apps/web/` is an installable, offline PWA. The WASM build of `txtodo-core` runs in a Web Worker
(so parsing/tokenizing never blocks the UI thread); OPFS is the local store; sync rides the
`txtodo-sync` protocol over a WebSocket to the M8 relay (browsers cannot do iroh QUIC). Like every
client it is thin — the JS never parses a line; it renders `tokenize` spans from the worker.

## Design

The worker owns the core, the store, and the wire; the main thread only paints and dispatches.

```ts
// apps/web/src/worker/core.ts — WASM core in the worker; the UI never parses a line itself.
import init, { tokenize, parse_line } from "../../pkg/txtodo_ffi";  // wasm-bindgen build of txtodo-ffi
await init();
const spans: Span[] = tokenize(raw);   // Span { kind: SpanKind, start, end }, 9 §3.1 names

// apps/web/src/worker/db.ts — OPFS is the local store: append-only op log + projection cache.
// Ref: https://developer.mozilla.org/en-US/docs/Web/API/File_System_API/Origin_private_file_system
const root = await navigator.storage.getDirectory();                // OPFS root handle
const oplog = await root.getFileHandle("oplog.bin", { create: true });
const proj  = await root.getFileHandle("projection.txt", { create: true });
async function appendOp(bytes: Uint8Array): Promise<void> { /* append-only, flush, never truncate */ }
```

```ts
// apps/web/src/worker/sync.ts — WebSocket carrier for the M4 txtodo-sync protocol.
// Browsers cannot open QUIC, so Hello/Want/Ops/Ack (postcard) runs over WS to the M8 relay,
// which stores only ciphertext blobs keyed by group+device (design §4.5, §4.6).
const ws = new WebSocket(WSS_ENDPOINT);        // wss://…/v1/sync
ws.onopen  = () => send({ Hello: { device, group, heads } });   // then Want{missing ranges}
ws.onmessage = async (e) => {
  const ops = decryptAndVerify(await e.data.arrayBuffer());     // XChaCha20-Poly1305 + Ed25519, design §4.6
  await applyAll(ops);                                          // same reconciler path as every client
};
```

- **UI model is the file** (design §7): render `todo.txt` as highlighted, numbered lines; `id:`
  hidden by default with a toggle; single tap → edit popover (raw line + chips), double tap →
  detail view (`ref:` directory, notes, recursive sub-list). The desktop M7 interaction spec applies.
- **Service worker** caches the app shell + the `.wasm`; `manifest.webmanifest` makes it installable.
  The SW does not cache todo data — OPFS is the only local store (data must never be stale in a cache).
- **Offline:** ops queue in OPFS; on reconnect the worker sends `Want{missing ranges}` and receives
  the tail. A local projection renders immediately from OPFS, so the app is usable offline.
- **Keys:** browsers have no OS keystore; the group key lives in IndexedDB as a `CryptoKey`, and
  enrolling a new device still goes through QR/SAS pairing (design §4.6) — the PWA is a peer like
  any other, not a special case.

## Placement/dependencies

- New directory `apps/web/` (TS + Vite/`wasm-pack`), non-frozen; no Rust crate changes and root
  `Cargo.toml` is untouched. Reuses `txtodo-ffi`'s wasm-bindgen target (already in its scope) — no
  new crates, no new frozen-path writes.
- Depends on M8 `relay/` (the WebSocket endpoint + ciphertext blob store) and the M4
  `sync-protocol-frames` / `sync-crypto-envelope` wire format. The wasm crypto must match
  `sync-crypto-envelope` byte-for-byte (postcard + XChaCha20-Poly1305 + Ed25519).

## Edge cases & invariants

- OPFS is origin-scoped and can be evicted by the browser under storage pressure — call
  `navigator.storage.persist()` and treat eviction as "re-fetch full snapshot from peers", never
  data loss (files are the truth; a peer still holds them).
- WebSocket drops: reconnect with bounded backoff, re-run `Hello`/`Want` so no op range is skipped;
  the relay re-serves ciphertext blobs, so the PWA never trusts a bare resend.
- Offline edits are never lost: appended to OPFS *before* any send attempt; the same no-loss
  invariant as every client.
- The SW must not serve a cached copy of `projection.txt` — data reads go to OPFS only, or a stale
  cached list lies to the user.
- Invariant (assert the negative): no JS module imports a todo.txt parser; all span/validation work
  is a `tokenize`/`parse_line` call into the worker.

## Acceptance

- `manifest.webmanifest` + service worker install the PWA; it loads and edits offline (airplane mode).
- `tokenize` in the worker returns byte-identical spans to `txtodo_core::tokenize` for a corpus line.
- Two browsers (or browser + daemon) converge via the relay over WebSocket; offline edits flush on
  reconnect with no loss.
- Service worker update replaces the shell without touching OPFS data.

## References

- design §7 Web PWA row + §4.5 relay carrier + §4.6 trust (txtodo-design.md); plan M10
- OPFS: https://developer.mozilla.org/en-US/docs/Web/API/File_System_API/Origin_private_file_system
- wasm-bindgen: https://rustwasm.github.io/wasm-bindgen/ · Service workers: https://developer.mozilla.org/en-US/docs/Web/API/Service_Worker_API
