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
4. **Store: a trait, with an OPFS impl (append-only op log plus snapshot).**
   - Alternative: SQLite compiled to wasm with an OPFS VFS. `libsqlite3-sys` 0.38 only supports
     WASI, so that means a C toolchain in the wasm build and a second SQLite.
   - Picked the trait: it also gives an in-memory store for tests.
5. **Keys: IndexedDB, raw bytes wrapped by a non-extractable WebCrypto AES-GCM key.**
   - Alternative: plain IndexedDB.
   - Picked wrapping: a copied profile folder alone does not leak the group key. Gap: script on the
     same origin can still unwrap it.
6. **One tab owns the data (Web Locks); other tabs say "open in another tab".**
   - Alternative: a leader tab with followers proxied over BroadcastChannel.
   - Picked v1 single owner: an OPFS sync handle is exclusive, so one owner is the honest start.
7. **Hosting: static files on GitHub Pages.**
   - Alternative: the planned Kamal droplet (`docs/relay-deployment-plan.md`), which is still
     `@human` and not up.
   - Picked Pages: static, HTTPS (which service workers and OPFS need), no server. Turning it on is
     outward-facing, so it stays a human step.

## Design

```text
main thread (desktop Svelte UI, --mode web)
  └─ tauriShim → webShim.ts ── postMessage(cmd, args) ──┐
dedicated Worker (txtodo-web, wasm-bindgen)             ▼
  ├─ commands: the desktop's file / notes / workspace / universal / pairing commands
  ├─ txtodo-engine: DocState, apply, mutation, reconcile, text_history (moved out of the daemon)
  ├─ store: OpfsStore (createSyncAccessHandle; append-only oplog + snapshot)
  ├─ keys: IdbKeyStore (wrapped bytes) behind txtodo-sync's KeyStore trait
  └─ sync: iroh Endpoint (wasm_browser) → relay wss → daemon, ALPN txtodo/sync/1, protocol 5
service worker: caches the shell + .wasm only, never list data
```

- **New crates (need an ADR and a human sign-off):**
  - `txtodo-engine`: the daemon's pure modules plus the pure half of the actor glue (`on_sync_run`
    dedupe, park and commit; `replayable_ops`). The daemon and the web host then share one engine,
    not two copies.
  - `txtodo-web`: the wasm host.
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

## Risks to spike first

- iroh `wasm_browser` in a dedicated Worker, not a window: timers and `web_sys` globals. Does
  `ring` build for wasm32 (it needs a wasm-capable clang)? What is the .wasm size?
- Does the n0 public relay accept browser WebSocket clients (Origin checks, rate limits)?
- Do the engine and model compile for wasm32 once moved? Expect getrandom, `Instant`, and
  `duplicates.rs:49`'s proto type.
- iOS Safari: storage for a site not added to the home screen can be cleared after 7 days without
  use. Installed web apps are exempt, so the app must push installing on iOS.

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
- GitHub Pages: https://docs.github.com/en/pages
