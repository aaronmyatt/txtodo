# Web PWA: a relay-only sync peer in the browser (plan M10, design §5, §7)

## Goal

An installable PWA that works offline and syncs with the user's other devices: the phone client,
now that M9 (native iOS/Android) is parked. It is a real peer. It holds the op log, edits offline,
and merges what other devices send. It is not a remote view of some daemon.

## What changed since the 2026-09-12 plan

That plan (git `f7f8225d`) is out of date in three places:

- "Browsers cannot do iroh." They can now. iroh 1.2 builds for `wasm32-unknown-unknown` with a
  `wasm_browser` cfg. There is no direct path and no hole punching: QUIC runs inside the relay's
  WebSocket (`iroh-1.2.0/Cargo.toml:407-437`, `iroh-relay src/client.rs:387-416`). A browser can
  speak to a daemon over the same `txtodo/sync/1` ALPN and protocol 5. So no new wire format and
  no WebSocket protocol of our own are needed.
- "No Rust crate changes; apply ops through the same reconciler." The reconciler is not in core
  or ffi. It lives in `txtodo-daemon`: about 4.6k LOC of pure code across 30 files (`state*`,
  `fields`, `text_history`, `mutation*`, `reconcile*`, `identity_*`, `duplicates`, ...), wrapped in
  about 5.3k LOC of actor glue tied to SQLite and tokio. There is no store trait.
- "A new main view." The desktop frontend already runs in a plain browser (`npm run dev` with the
  mock). Playwright already drives it against a real daemon through an HTTP shim (`--mode e2e`).
  A third UI to keep at parity would be waste.

Also new: the `relay/` blob mailbox is undeployed and has no auth (ADR 0027). Every message is
sealed with the group key (`lan_session_shared.rs:11-16`). Op signatures use a key derived from
the group key, not per-device keys (`lan_op_signing.rs:6-13`).

## Decisions (agent picks, 2026-10-17; the human can overturn any)

Accepted as ADR 0042 (`docs/adr/0042-web-pwa-relay-only-peer.md`), with decision 7 changed
the same day (below).

1. **Real peer, not a thin client.**
   - Thin client: the browser drives the user's daemon over iroh. Cheap, but there is no offline
     use and a daemon must be online for every read.
   - Picked the peer: the line's own spec is offline plus OPFS.
2. **Reuse the desktop frontend.**
   - Alternative: a new `apps/web` UI. It would be a third client for `client-parity.toml`.
   - Picked a `web` Vite mode in `apps/desktop`. It aliases `@tauri-apps/api/*` to a Worker shim,
     the same trick `--mode e2e` uses (`vite.config.ts:25-34`).
3. **Carrier: iroh `wasm_browser`, relay-only.**
   - Alternative: our own WebSocket protocol to `relay/`. That needs a new wire format, a deploy,
     and auth that `relay/` lacks.
   - Picked iroh: daemons need no new code path.
4. **Store: `txtodo-store` unchanged, on SQLite built for wasm, in OPFS.** (Amended after
   spike 2.)
   - First pick was a store trait plus an OPFS append-only log, because I thought SQLite built
     only for WASI. That was wrong. rusqlite 0.40 on `wasm32-unknown-unknown` links
     `sqlite-wasm-rs` (SQLite compiled to wasm). `sqlite-wasm-vfs`'s `sahpool` VFS keeps the file
     in OPFS from a dedicated Worker.
   - Fallback: the trait, if the VFS fails on Safari or under eviction.

5. **Keys: IndexedDB, raw bytes wrapped by a non-extractable WebCrypto AES-GCM key.**
   - Alternative: plain IndexedDB.
   - Picked wrapping: a copied profile folder alone does not leak the group key. Gap: script on the
     same origin can still unwrap it.
6. **One tab owns the data (Web Locks); other tabs say "open in another tab".**
   - Alternative: a leader tab with followers proxied over BroadcastChannel.
   - Picked v1 single owner: an OPFS sync handle is exclusive, so one owner is the honest start.
7. **Hosting: Cloudflare Workers static assets, on its own `workers.dev` origin.**
   - First pick was GitHub Pages. Dropped: the site would be `aaronmyatt.github.io/txtodo/`, one
     origin with the owner's 21 other Pages sites. OPFS and IndexedDB are per origin, so script on
     any of them could read the lists and unwrap the group key. Pages also sets no response headers.
   - Alternative: GitHub Pages under a new org (its own origin). Still no headers, so no real CSP
     and no `frame-ancestors`.
   - Picked Cloudflare: its own origin, plus a `_headers` file for the CSP. Cloudflare now steers
     static sites to Workers rather than Pages. The owner makes the account and the API token
     (a secret), so that one step stays `@human`.

## Spike results (2026-10-17)

Throwaway code was in the session scratchpad, not the repo. The recipe is below, so it can be
rebuilt.

- **Build env:** the C parts (ring for iroh's TLS, SQLite) need a clang that targets wasm32.
  - Apple clang cannot. Homebrew `llvm@21` can:
    `CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm@21/bin/clang`,
    `AR_wasm32_unknown_unknown=…/llvm-ar`.
  - CI needs a clang with wasm32 too (Ubuntu's apt clang has it).
  - `wasm-bindgen` CLI 0.2.128 is already installed: `--target web`.
- **Spike 1 (iroh in a Worker):**
  - Setup: `iroh = "=1.2.0"` with `default-features = false, features = ["tls-ring"]`,
    `Endpoint::builder(Minimal).relay_mode(RelayMode::Custom(<n0 url>))`, then
    `connect(EndpointAddr::new(id).with_relay_url(url), ALPN)`.
  - Result: a dedicated module Worker, and the page itself, reached a native iroh endpoint homed
    on `use1-1.relay.n0.iroh.link`. 5 of 5 runs echoed. Bind took 3-4 ms, connect about 1.5 s,
    one round trip 0.6-0.8 s (this machine to us-east).
  - Size (`opt-level = "z"`, LTO): 2.52 MB wasm, 0.95 MB gzip, 0.75 MB brotli.
  - Gap: an echo, not a sealed `Hello`. A real `Hello` needs `txtodo-sync` built for wasm (its own
    line).
- **Spike 2 (what builds for wasm32):**
  - `cargo check --target wasm32-unknown-unknown`: core, model, crdt and store pass.
  - proto fails: tonic's default transport pulls tokio net, then mio.
  - sync fails: its own tokio `net`/`rt-multi-thread` features plus mdns-sd pull mio. iroh itself
    is fine.
  - The daemon's 30 engine modules (plus `expected.rs`), included by path into a scratch crate,
    build with only two snags:
    - `duplicates.rs:49` builds a proto type; that belongs in the gRPC layer.
    - `sync_park.rs:32` uses `sync_ops::apply_leniently`, which is pure, so it moves with the
      engine.
  - `expected.rs` and `Clock` use `Instant`. They compile but would panic at run time (the Clock
    line).
- **Spike 2b (store in a Worker):**
  - Setup: `sqlite_wasm_vfs::sahpool::install::<sqlite_wasm_rs::WasmOsCallback>(&Default::default(),
    true)`, then `Store::open("/x.db")` unchanged.
  - It ran all 9 migrations. Ops persisted across a reload, and a fresh browser context started
    empty.
  - WAL is not available there. SQLite falls back to `journal_mode=delete` without an error, which
    is fine for the one connection a Worker holds.
  - Cost: 1.05 MB wasm, 0.47 MB gzip.

## Design

```text
main thread (desktop Svelte UI, --mode web)
  └─ tauriShim → webShim.ts ── postMessage(cmd, args) ──┐
dedicated Worker (txtodo-web, wasm-bindgen)             ▼
  ├─ commands: the desktop's file / notes / workspace / universal / pairing commands
  ├─ txtodo-engine: DocState, apply, mutation, reconcile, text_history (moved out of the daemon)
  ├─ store: txtodo-store as is; SQLite in wasm, sahpool VFS on OPFS (createSyncAccessHandle)
  ├─ keys: IdbKeyStore (wrapped bytes) behind txtodo-sync's KeyStore trait
  └─ sync: iroh Endpoint (wasm_browser) → relay wss → daemon, ALPN txtodo/sync/1, protocol 5
service worker: caches the shell + .wasm only, never list data
```

- **New crates (need an ADR and a human sign-off):**
  - `txtodo-engine`: the daemon's pure modules plus the pure half of the actor glue (`on_sync_run`
    dedupe, park and commit; `replayable_ops`). The daemon and the web host then share one engine,
    not two copies.
  - `txtodo-web`: the wasm host.
  - Not a store trait: `txtodo-store` runs as is (decision 4).
  - Root `Cargo.toml` and `.github/**` are frozen paths; `.claude/budgets.json` needs both
    crates' `allowedDeps`.
- **txtodo-sync:** move `WorkspaceId` to `txtodo-model`. Today `message.rs:42` pulls in rusqlite
  through `txtodo-store`. A `native` feature (on by default) gates iroh-tokio, mdns-sd, keyring and
  the fs carriers. getrandom needs `wasm_js` (0.3) and `js` (0.2).
- **Clock:** `Clock::now_instant()` panics on wasm. Use a wall-ms monotonic source there.
- **Parity:** the web build is the desktop frontend, so `client-parity.toml` gains an optional
  `web` field, for `na` rows only (tray, quick-add, pin on top, upgrade, folder dialog). A missing
  field means "same as desktop". `keys.parity.test.ts` checks that `na` rows are hidden in a web
  build.
- **Workspaces:** there are no folders. A browser workspace is a Remote mirror, accepted through
  the existing workspace offers (ADR 0021: one group, many workspaces, one link).

## Risks

- Answered by the spikes: iroh works in a Worker; ring builds with a wasm-capable clang; the n0
  relay accepts a browser client (no Origin refusal seen); the engine compiles, with two snags.
- Still open:
  - Safari: OPFS sync handles, and iroh's WebSocket path, were tested in Chromium only.
  - Size: about 3.5 MB of wasm before the engine (about 1.4 MB gzip). That is fine for an
    installed app, slow on a first visit over mobile data.
  - iOS Safari: storage for a site not added to the home screen can be cleared after 7 days
    without use. Installed web apps are exempt, so the app must push installing on iOS.

## Known gaps, by design

- Sync happens only while another device is online. The relay forwards; it does not store. The
  long-term fix is the `relay/` mailbox, which is out of scope here.
- Eviction (storage pressure, or Safari's cap) loses ops that were never synced. The UI shows the
  count of unsynced ops; `persist()` is requested.
- Notes assets (images, ADR 0014) are not in v1.

## Acceptance

- Installs, loads and edits offline; reload offline shows the edits (OPFS, not the SW cache).
- `state_goldens` and the converge tests pass under `wasm-bindgen-test`: byte-identical text.
- A browser and a daemon converge over the relay. Offline edits flush on reconnect with no loss.
- A service worker update replaces the shell without touching OPFS data.

## References

- design §5 (:321), §7 (:428), §4.5, §4.6; ADRs 0021, 0026, 0027, 0031, 0035, 0038
- iroh 1.2: https://docs.rs/iroh/1.2.0/iroh/ (browser support: the crate's `wasm_browser` cfg)
- OPFS: https://developer.mozilla.org/en-US/docs/Web/API/File_System_API/Origin_private_file_system
- Sync access handle (Worker only):
  https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle/createSyncAccessHandle
- persist(): https://developer.mozilla.org/en-US/docs/Web/API/StorageManager/persist
- Web Locks: https://developer.mozilla.org/en-US/docs/Web/API/Web_Locks_API
- wrapKey: https://developer.mozilla.org/en-US/docs/Web/API/SubtleCrypto/wrapKey
- Service workers: https://developer.mozilla.org/en-US/docs/Web/API/Service_Worker_API
- Manifest: https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Manifest
- getrandom on wasm: https://docs.rs/getrandom/latest/getrandom/#webassembly-support
- wasm-bindgen-test: https://docs.rs/wasm-bindgen-test
- Vite modes: https://vite.dev/guide/env-and-mode
- WebKit 7-day storage cap: https://webkit.org/blog/10218/full-third-party-cookie-blocking-and-more/
- Workers static assets headers: https://developers.cloudflare.com/workers/static-assets/headers/
